//! unx604_cpioiter —— UNX-F0604 cpio 条目遍历（110 字节头）。
//!
//! 判据原文摘录（docs/Varix/CoRun Varix STAR II · Unxreal 主册 · UNX-A1-B31 批）：
//!   | UNX-F0604 | cpio 条目遍历（110 字节头） | 240 | 已深化 | 遍历账齐（n 条目=n
//!   推算数），头字段账（ino/mode/filesize/namesize 等 8 要素）逐条核 |
//! 定位原文：UNX-A1-B31 · initrd 只读桩（F601–F620 · 20 条）批表第 4 行；
//!   批头防重注：「initrd 归档只读账（识别/遍历/查找/红线）归本批独占；文件系统
//!   语义（写/挂载/缓存）属后续域（本批纯只读桩）」。
//!
//! 本模块落地（全手工定制真实子系统，零 crate:: 依赖，no_std + alloc）：
//!   1. `CpioNewcHeader` 110 字节 newc 头真实解析：6 字节魔数 "070701" + 13 个
//!      8 位 ASCII 十六进制整数字段（c_ino/c_mode/c_uid/c_gid/c_nlink/c_mtime/
//!      c_filesize/c_devmajor/c_devminor/c_rdevmajor/c_rdevminor/c_namesize/
//!      c_check），逐字段手写十六进制解析（零依赖），短头/越界拒收。
//!   2. `CpioIterator` 安全遍历：游标推进 = 110 头 + 名区（含 NUL，4 对齐垫）+
//!      数据区（4 对齐垫）；对齐推进真实实现（垫字节必须为 0）；遍历账（条目
//!      序号连续 + 总字节守恒）；namesize=0 拒收；名区末字节非 NUL 拒收。
//!   3. 头字段账 8 要素逐条核：c_ino 登记账/c_mode 类型闭集+权限域/c_uid+c_gid
//!      16 位域/c_nlink 合法域/c_mtime 哨兵账注/c_filesize 数据一致性/c_namesize
//!      名区一致性/c_check 零值核——逐条留痕入 FieldAuditReport。
//!   4. 边界防线：数据区越出缓冲拒收（0x14B5）/游标越过末尾拒收（0x14B6）/
//!      空归档显性空态（EmptyArchive 事件，非错误）。
//!   5. TRAILER 处理："TRAILER!!!" 终止符识别 + 尾后残余字节账注（残余=0 合法）。
//!   6. 抽样对拍：遍历重建（canonical 重编码）vs 原始字节双 FNV-1a 32 指纹对拍
//!      + 往返一致自证（重编码字节流与原档逐字节等长等值）。
//!   7. 事件账 + health() + 双跑确定性对拍（同档两次独立全走序指纹恒等）+
//!      注入面（魔数坏/对齐垫坏/越界坏/名 NUL 坏/namesize 零 五类检出演练）。
//!
//! 纪律：内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert
//!   除外）；可失败全走 Result<T, CpioIterError>；错误枚举逐变体 detail() 中文
//!   可操作诊断；溢出显性 saturating/wrapping/checked；>64KB 结构体禁上栈（全部
//!   大件走 Vec 堆）；const fn 内零 slice::get/Vec 索引/Ord::min,max/derive ==
//!   /for 循环。诊断码段独占 0x14B0~0x14BF（先 grep 确认空闲，16 码互斥）。

use alloc::{format, string::String, vec::Vec};

// ==========================================================================
// 版本与诊断码段
// ==========================================================================

/// 模块版本。
pub const V604I_VERSION: &str = "v604-cpioiter-v1";

/// 诊断段基址（0x14B0）。段 0x14B0~0x14BF 归本模块独占（grep 全仓确认为空闲段）。
pub const V604I_SEG_BASE: u16 = 0x14B0;

/// 诊断段末址（0x14BF）。
pub const V604I_SEG_END: u16 = 0x14BF;

/// 诊断码（16 码互斥，覆盖本模块全部可诊断条件；正常事件不占诊断码，
/// 走 IterEventKind 标签，异常事件携带 Option<u16> 指向本闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V604iCode {
    /// 0x14B0 头短于 110 字节（newc 头被截断）。
    HeadShort,
    /// 0x14B1 魔数非 "070701"（含坏魔数注入）。
    MagicMismatch,
    /// 0x14B2 整数字段含非十六进制字符（8 位 ASCII hex 域被破坏）。
    HexNonHex,
    /// 0x14B3 c_namesize = 0（newc 规定名区至少含 1 个 NUL）。
    NamesizeZero,
    /// 0x14B4 名区末字节非 NUL（名字未按规格以 NUL 终止）。
    NameNulMissing,
    /// 0x14B5 名区/数据区越出归档缓冲（越界注入或截断档）。
    DataOverrun,
    /// 0x14B6 游标越过末尾（残余不足一个完整 110 字节头）。
    CursorPastEnd,
    /// 0x14B7 c_mode 类型位落出 S_IFMT 七类闭集。
    ModeTypeUnknown,
    /// 0x14B8 c_nlink 落出合法域 [1, 0xFFFF]。
    NlinkIllegal,
    /// 0x14B9 对齐垫字节非零（canonical newc 垫必须为 0；对齐错注入）。
    PadNonZero,
    /// 0x14BA 遍历账失守（条目序号不连续 / 总字节不守恒 / 双跑计数失配）。
    LedgerMiss,
    /// 0x14BB 头字段账 8 要素失配（uid/gid 域、mtime 哨兵、名字节域等）。
    FieldAuditMiss,
    /// 0x14BC 归档未以 TRAILER!!! 终止（尾核失守）。
    TrailerMissing,
    /// 0x14BD 双 FNV 指纹对拍失配（重建 vs 原始字节）。
    FingerprintMiss,
    /// 0x14BE c_check 非 0（newc "070701" 格式规定 check 恒为 0）。
    CheckFieldNonZero,
    /// 0x14BF 重入/状态机守卫（游标非单调推进等内部违例）。
    ReentryGuard,
}

impl V604iCode {
    /// 全量闭集（与段内 16 码一一对应，序即码升序）。
    pub const ALL: [V604iCode; 16] = [
        V604iCode::HeadShort,
        V604iCode::MagicMismatch,
        V604iCode::HexNonHex,
        V604iCode::NamesizeZero,
        V604iCode::NameNulMissing,
        V604iCode::DataOverrun,
        V604iCode::CursorPastEnd,
        V604iCode::ModeTypeUnknown,
        V604iCode::NlinkIllegal,
        V604iCode::PadNonZero,
        V604iCode::LedgerMiss,
        V604iCode::FieldAuditMiss,
        V604iCode::TrailerMissing,
        V604iCode::FingerprintMiss,
        V604iCode::CheckFieldNonZero,
        V604iCode::ReentryGuard,
    ];

    /// 码值（与声明序恒等，段内连续升序）。
    pub const fn code(self) -> u16 {
        match self {
            V604iCode::HeadShort => 0x14B0,
            V604iCode::MagicMismatch => 0x14B1,
            V604iCode::HexNonHex => 0x14B2,
            V604iCode::NamesizeZero => 0x14B3,
            V604iCode::NameNulMissing => 0x14B4,
            V604iCode::DataOverrun => 0x14B5,
            V604iCode::CursorPastEnd => 0x14B6,
            V604iCode::ModeTypeUnknown => 0x14B7,
            V604iCode::NlinkIllegal => 0x14B8,
            V604iCode::PadNonZero => 0x14B9,
            V604iCode::LedgerMiss => 0x14BA,
            V604iCode::FieldAuditMiss => 0x14BB,
            V604iCode::TrailerMissing => 0x14BC,
            V604iCode::FingerprintMiss => 0x14BD,
            V604iCode::CheckFieldNonZero => 0x14BE,
            V604iCode::ReentryGuard => 0x14BF,
        }
    }

    /// 码值反查（无码命中返回 None——诊断面闭环自查）。
    pub const fn from_code(value: u16) -> Option<V604iCode> {
        match value {
            0x14B0 => Some(V604iCode::HeadShort),
            0x14B1 => Some(V604iCode::MagicMismatch),
            0x14B2 => Some(V604iCode::HexNonHex),
            0x14B3 => Some(V604iCode::NamesizeZero),
            0x14B4 => Some(V604iCode::NameNulMissing),
            0x14B5 => Some(V604iCode::DataOverrun),
            0x14B6 => Some(V604iCode::CursorPastEnd),
            0x14B7 => Some(V604iCode::ModeTypeUnknown),
            0x14B8 => Some(V604iCode::NlinkIllegal),
            0x14B9 => Some(V604iCode::PadNonZero),
            0x14BA => Some(V604iCode::LedgerMiss),
            0x14BB => Some(V604iCode::FieldAuditMiss),
            0x14BC => Some(V604iCode::TrailerMissing),
            0x14BD => Some(V604iCode::FingerprintMiss),
            0x14BE => Some(V604iCode::CheckFieldNonZero),
            0x14BF => Some(V604iCode::ReentryGuard),
            _ => None,
        }
    }

    /// 中文诊断名（短）。
    pub const fn name(self) -> &'static str {
        match self {
            V604iCode::HeadShort => "头短于 110 字节",
            V604iCode::MagicMismatch => "魔数非 070701",
            V604iCode::HexNonHex => "整数字段含非十六进制字符",
            V604iCode::NamesizeZero => "namesize 为 0",
            V604iCode::NameNulMissing => "名区末字节非 NUL",
            V604iCode::DataOverrun => "名区/数据区越出归档缓冲",
            V604iCode::CursorPastEnd => "游标越过末尾",
            V604iCode::ModeTypeUnknown => "mode 类型位落出闭集",
            V604iCode::NlinkIllegal => "nlink 落出合法域",
            V604iCode::PadNonZero => "对齐垫字节非零",
            V604iCode::LedgerMiss => "遍历账失守",
            V604iCode::FieldAuditMiss => "头字段账 8 要素失配",
            V604iCode::TrailerMissing => "归档未以 TRAILER 终止",
            V604iCode::FingerprintMiss => "双 FNV 指纹对拍失配",
            V604iCode::CheckFieldNonZero => "c_check 非 0",
            V604iCode::ReentryGuard => "重入/状态机守卫",
        }
    }

    /// 中文可操作诊断（长——修复指引）。
    pub const fn detail(self) -> &'static str {
        match self {
            V604iCode::HeadShort => "归档缓冲不足 110 字节，无法容纳一个完整 newc 头：检查 initrd 基址/长度交接（UNX-F0601）是否把归档截短，或归档偏移是否错位。",
            V604iCode::MagicMismatch => "条目头前 6 字节不等于 \"070701\"：确认归档为 newc 格式（-H newc 生成），或排查前条目游标推进是否多算/少算字节（对齐垫长度）。",
            V604iCode::HexNonHex => "13 个 8 位十六进制整数字段中出现非 [0-9a-fA-F] 字符：头部被覆写或生成器损坏；用重建/重展开归档定位坏条目偏移。",
            V604iCode::NamesizeZero => "c_namesize = 0 违反 newc 规格（名区至少含终止 NUL，故 namesize ≥ 1）：生成器 bug 或头部字段错位；核对字段序 c_namesize 为第 12 字段。",
            V604iCode::NameNulMissing => "名区最后一个字节不是 NUL：名字未按规格终止，路径边界已不可信；拒收该条目并核对生成器是否少写 1 字节 NUL。",
            V604iCode::DataOverrun => "名区或数据区终点越出归档缓冲（长度交接过短或 c_filesize 被注水）：核对 UNX-F0601 长度账与 c_filesize 字段，截断档需重新展开。",
            V604iCode::CursorPastEnd => "尾部残余不足一个完整 110 字节头（游标将越过末尾）：归档被截断或尾后混杂异格式数据；核对归档长度与 TRAILER 位置。",
            V604iCode::ModeTypeUnknown => "c_mode 的高 3 位类型域不在 S_IFMT 七类闭集（fifo/chr/dir/blk/reg/lnk/sock）内：头部损坏或非 cpio 语义位混入；按拒收处理。",
            V604iCode::NlinkIllegal => "c_nlink 不在 [1, 0xFFFF]：硬链接数为 0 或越域均非法；检查生成器对目录/文件 nlink 的赋值。",
            V604iCode::PadNonZero => "对齐垫字节非零（canonical newc 垫必须写 0）：生成器在垫位写入垃圾数据，破坏逐字节往返一致；重生成归档。",
            V604iCode::LedgerMiss => "遍历账失守：条目序号不连续或累计消耗字节 ≠ 游标位置（Σ(110+名区垫+数据垫) ≠ 消耗）；先核对单条目 span 计算再查游标推进。",
            V604iCode::FieldAuditMiss => "头字段账 8 要素（ino/mode/uid+gid/nlink/mtime/filesize/namesize/check）逐条核出现失配：见 FieldAuditReport 的 element_pass 位图定位失配要素。",
            V604iCode::TrailerMissing => "归档未以 \"TRAILER!!!\" 条目终止：尾部数据被截断或游标提前越界；核对归档总长与最后条目 span。",
            V604iCode::FingerprintMiss => "遍历重建（canonical 重编码）与原始字节双 FNV-1a 指纹对拍失配：解码或重编码实现有损；对比重建缓冲与原档首个差异字节定位。",
            V604iCode::CheckFieldNonZero => "c_check 非 0：newc（\"070701\"）格式规定 check 恒为 0，仅 \"070702\"（newcrc）才写校验和；确认归档格式族。",
            V604iCode::ReentryGuard => "遍历器状态机违例（游标非单调推进/结束后再入）：检查是否存在并发使用同一遍历器或 span 计算回退。",
        }
    }
}

// 编译期账：段内 16 码互斥、连续、首尾对齐段界。
const _: () = assert!(V604iCode::ALL.len() == 16);
const _: () = assert!(V604iCode::ALL[0].code() == V604I_SEG_BASE);
const _: () = assert!(V604iCode::ALL[15].code() == V604I_SEG_END);

// ==========================================================================
// 统一错误类型
// ==========================================================================

/// 遍历域统一错误（Copy 小件，逐错带条目序号与归档内偏移留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CpioIterError {
    code: V604iCode,
    entry_index: u32,
    offset: u32,
}

impl CpioIterError {
    /// 构造（entry_index 为出错时正在解析的条目序号，offset 为归档内字节偏移）。
    pub const fn new(code: V604iCode, entry_index: u32, offset: u32) -> Self {
        CpioIterError {
            code,
            entry_index,
            offset,
        }
    }

    /// 诊断码。
    pub const fn code(&self) -> V604iCode {
        self.code
    }

    /// 段内数值诊断码（u16 形态，供事件账/上报面）。
    pub const fn code_value(&self) -> u16 {
        self.code.code()
    }

    /// 出错条目序号。
    pub const fn entry_index(&self) -> u32 {
        self.entry_index
    }

    /// 出错归档内偏移。
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    /// 中文可操作诊断（透传码位 detail）。
    pub const fn detail(&self) -> &'static str {
        self.code.detail()
    }

    /// 带定位的完整中文描述（诊断打印面用）。
    pub fn describe(&self) -> String {
        format!(
            "[{:04X}] {} @条目{} @偏移0x{:X}：{}",
            self.code.code() as u16,
            self.code.name(),
            self.entry_index,
            self.offset,
            self.code.detail()
        )
    }
}

// ==========================================================================
// 手写十六进制解析（零依赖）
// ==========================================================================

/// 魔数 "070701"（cpio newc 格式族魔数，GNU cpio copyin.c 规格）。
pub const NEWC_MAGIC: [u8; 6] = *b"070701";

/// 魔数长度。
pub const MAGIC_LEN: usize = 6;

/// 单个整数字段十六进制长度（newc 规定每整数字段 8 位 ASCII hex）。
pub const FIELD_HEX_LEN: usize = 8;

/// 单个十六进制字符取值（'0'..'9'/'a'..'f'/'A'..'F' → 0..15，否则 None）。
pub const fn hex_digit_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 十六进制半字节取 ASCII 小写字符。
pub const fn hex_char(nibble: u32) -> u8 {
    if nibble < 10 {
        b'0' + nibble as u8
    } else {
        b'a' + (nibble - 10) as u8
    }
}

/// 解析一个 8 位 ASCII 十六进制整数字段（大端聚合，0xFFFFFFFF 封顶不溢出）。
/// 长度非 8 或含非十六进制字符 → HexNonHex。
pub fn parse_hex8(bytes: &[u8]) -> Result<u32, V604iCode> {
    if bytes.len() != FIELD_HEX_LEN {
        return Err(V604iCode::HexNonHex);
    }
    let mut value: u32 = 0;
    let mut i: usize = 0;
    while i < FIELD_HEX_LEN {
        let digit = match hex_digit_value(bytes[i]) {
            Some(d) => d,
            None => return Err(V604iCode::HexNonHex),
        };
        value = (value << 4) | (digit as u32);
        i += 1;
    }
    Ok(value)
}

/// 编码一个 u32 为 8 位 ASCII 小写十六进制（定长输出，无分支越界）。
pub const fn encode_hex8(value: u32) -> [u8; 8] {
    let mut out = [b'0'; FIELD_HEX_LEN];
    let mut i: usize = 0;
    while i < FIELD_HEX_LEN {
        let shift = (7 - i) * 4;
        let nibble = (value >> shift) & 0xF;
        out[i] = hex_char(nibble);
        i += 1;
    }
    out
}

/// 把一个整数字段原位写回归档缓冲（注入面/修复面共用）。
pub fn write_hex8_into(buf: &mut [u8], field_offset: usize, value: u32) -> bool {
    if field_offset + FIELD_HEX_LEN > buf.len() {
        return false;
    }
    let enc = encode_hex8(value);
    let mut i: usize = 0;
    while i < FIELD_HEX_LEN {
        buf[field_offset + i] = enc[i];
        i += 1;
    }
    true
}

// ==========================================================================
// newc 格式常量（来源逐条注释）
// ==========================================================================

/// newc 条目头总长 = 6 魔数 + 13 × 8 hex = 110 字节（GNU cpio newc 规格）。
pub const HEADER_LEN: usize = MAGIC_LEN + 13 * FIELD_HEX_LEN;

/// 归档内对齐界：名区与数据区均按 4 字节边界收口（newc 规格 + Linux 内核
/// initramfs unpack 实现）。设为可审计常量而非魔数。
pub const ARCHIVE_ALIGNMENT: u32 = 4;

/// 名区终止字节（NUL）。
pub const NAME_TERMINATOR: u8 = 0;

/// TRAILER 终止符条目名（GNU cpio 规格：以该名条目收档）。
pub const TRAILER_NAME: &str = "TRAILER!!!";

/// c_nlink 合法域上界（initramfs 生成器域：usr/gen_init_cpio 产档 nlink 均
/// 为小整数；0xFFFF 为账面封顶）。
pub const NLINK_MAX: u32 = 0xFFFF;

/// c_uid / c_gid 传统域上界（POSIX uid_t 16 位习惯域；initramfs 产档 id 均
/// 16 位域内，越域记失配拒收）。
pub const UID_GID_MAX: u32 = 0xFFFF;

/// 路径名长度预算上界（VFS PATH_MAX 折算 4096；namesize 含 NUL 不得越界）。
pub const MAX_NAME_BYTES: u32 = 4096;

/// S_IFMT 文件类型位掩码（POSIX sys/stat.h）。
pub const S_IFMT: u32 = 0o170000;
/// FIFO 类型位（POSIX S_IFIFO）。
pub const S_IFIFO: u32 = 0o010000;
/// 字符设备类型位（POSIX S_IFCHR）。
pub const S_IFCHR: u32 = 0o020000;
/// 目录类型位（POSIX S_IFDIR）。
pub const S_IFDIR: u32 = 0o040000;
/// 块设备类型位（POSIX S_IFBLK）。
pub const S_IFBLK: u32 = 0o060000;
/// 常规文件类型位（POSIX S_IFREG）。
pub const S_IFREG: u32 = 0o100000;
/// 符号链接类型位（POSIX S_IFLNK）。
pub const S_IFLNK: u32 = 0o120000;
/// 套接字类型位（POSIX S_IFSOCK）。
pub const S_IFSOCK: u32 = 0o140000;

/// 权限+特殊位域上界（setuid/setgid/sticky + rwx 三组 = 12 位 = 0o7777）。
pub const MODE_PERM_MASK: u32 = 0o7777;

// 编译期账：头长守恒式。
const _: () = assert!(HEADER_LEN == 6 + 13 * 8);
const _: () = assert!(HEADER_LEN == 110);

// ==========================================================================
// 文件类型闭集（S_IFMT 七类）
// ==========================================================================

/// 文件类型闭集判别（S_IFMT 七类 + Unknown 兜底，绝不发散）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    /// FIFO（0o010000）。
    Fifo,
    /// 字符设备（0o020000）。
    CharDev,
    /// 目录（0o040000）。
    Directory,
    /// 块设备（0o060000）。
    BlockDev,
    /// 常规文件（0o100000）。
    Regular,
    /// 符号链接（0o120000）。
    Symlink,
    /// 套接字（0o140000）。
    Socket,
    /// 落出闭集（诊断位，判据红）。
    Unknown,
}

impl FileType {
    /// 从 mode 高 3 位类型域判别。
    pub const fn from_mode(mode: u32) -> FileType {
        match mode & S_IFMT {
            S_IFIFO => FileType::Fifo,
            S_IFCHR => FileType::CharDev,
            S_IFDIR => FileType::Directory,
            S_IFBLK => FileType::BlockDev,
            S_IFREG => FileType::Regular,
            S_IFLNK => FileType::Symlink,
            S_IFSOCK => FileType::Socket,
            _ => FileType::Unknown,
        }
    }

    /// 闭集内序号（Unknown = 7，供直方图下标）。
    pub const fn id(self) -> usize {
        match self {
            FileType::Fifo => 0,
            FileType::CharDev => 1,
            FileType::Directory => 2,
            FileType::BlockDev => 3,
            FileType::Regular => 4,
            FileType::Symlink => 5,
            FileType::Socket => 6,
            FileType::Unknown => 7,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            FileType::Fifo => "FIFO",
            FileType::CharDev => "字符设备",
            FileType::Directory => "目录",
            FileType::BlockDev => "块设备",
            FileType::Regular => "常规文件",
            FileType::Symlink => "符号链接",
            FileType::Socket => "套接字",
            FileType::Unknown => "未知类型",
        }
    }

    /// 闭集内合法类别数（7）。
    pub const KNOWN_COUNT: usize = 7;
}

/// 权限位解码（rwx 三组 + setuid/setgid/sticky，真实 POSIX 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermissionBits {
    /// 原始 12 位（0o7777 域内）。
    pub raw: u32,
}

impl PermissionBits {
    /// 从 mode 低 12 位解码。
    pub const fn from_mode(mode: u32) -> PermissionBits {
        PermissionBits {
            raw: mode & MODE_PERM_MASK,
        }
    }

    /// setuid 位（0o4000）。
    pub const fn setuid(&self) -> bool {
        (self.raw & 0o4000) != 0
    }

    /// setgid 位（0o2000）。
    pub const fn setgid(&self) -> bool {
        (self.raw & 0o2000) != 0
    }

    /// sticky 位（0o1000）。
    pub const fn sticky(&self) -> bool {
        (self.raw & 0o1000) != 0
    }

    /// 三组 rwx 位取值（owner=0/group=1/other=2）。
    pub const fn rwx(&self, group_index: usize) -> u32 {
        let shift = match group_index {
            0 => 6,
            1 => 3,
            _ => 0,
        };
        (self.raw >> shift) & 0o7
    }

    /// 渲染 "rwxr-xr-x" 形态九字符（特殊位以大写 s/S/t/T 语义并入 rwx 位）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut gi: usize = 0;
        while gi < 3 {
            let bits = self.rwx(gi);
            let r = (bits & 0o4) != 0;
            let w = (bits & 0o2) != 0;
            let x = (bits & 0o1) != 0;
            if gi == 0 {
                if r {
                    out.push('r');
                } else if self.setuid() {
                    out.push('S');
                } else {
                    out.push('-');
                }
                if w {
                    out.push('w');
                } else {
                    out.push('-');
                }
                if x && self.setuid() {
                    out.push('s');
                } else if x {
                    out.push('x');
                } else if self.setuid() {
                    out.push('S');
                } else {
                    out.push('-');
                }
            } else if gi == 1 {
                if r {
                    out.push('r');
                } else if self.setgid() {
                    out.push('S');
                } else {
                    out.push('-');
                }
                if w {
                    out.push('w');
                } else {
                    out.push('-');
                }
                if x && self.setgid() {
                    out.push('s');
                } else if x {
                    out.push('x');
                } else if self.setgid() {
                    out.push('S');
                } else {
                    out.push('-');
                }
            } else {
                if r {
                    out.push('r');
                } else {
                    out.push('-');
                }
                if w {
                    out.push('w');
                } else {
                    out.push('-');
                }
                if x && self.sticky() {
                    out.push('t');
                } else if x {
                    out.push('x');
                } else if self.sticky() {
                    out.push('T');
                } else {
                    out.push('-');
                }
            }
            gi += 1;
        }
        out
    }
}

// ==========================================================================
// 对齐推进（真实实现）
// ==========================================================================

/// 求把 total 收口到 ARCHIVE_ALIGNMENT 需补的垫字节数（0..3）。
pub const fn pad_needed(total: u32) -> u32 {
    (ARCHIVE_ALIGNMENT - (total % ARCHIVE_ALIGNMENT)) % ARCHIVE_ALIGNMENT
}

/// 把绝对偏移收口到 4 字节边界（wrapping 防守，运行值恒有界）。
pub const fn align_up(offset: usize) -> usize {
    offset.wrapping_add(ARCHIVE_ALIGNMENT as usize - 1) & !(ARCHIVE_ALIGNMENT as usize - 1)
}

/// 校验 [start, start+len) 区间内垫字节全部为 0（canonical newc 要求）。
fn pad_bytes_zero(buf: &[u8], start: usize, len: usize) -> Option<bool> {
    if len == 0 {
        return Some(true);
    }
    let end = start.checked_add(len)?;
    if end > buf.len() {
        return None;
    }
    let mut i: usize = 0;
    while i < len {
        if buf[start + i] != 0 {
            return Some(false);
        }
        i += 1;
    }
    Some(true)
}

// ==========================================================================
// 110 字节头真实解析
// ==========================================================================

/// newc 13 个整数字段的字段名表（规格序，offset = 6 + 序号 × 8）。
pub const FIELD_NAMES: [&str; 13] = [
    "c_ino",
    "c_mode",
    "c_uid",
    "c_gid",
    "c_nlink",
    "c_mtime",
    "c_filesize",
    "c_devmajor",
    "c_devminor",
    "c_rdevmajor",
    "c_rdevminor",
    "c_namesize",
    "c_check",
];

/// 字段在 110 头内的字节偏移表（编译期常数，供定位/注入面）。
pub const FIELD_OFFSETS: [usize; 13] = [
    6,
    14,
    22,
    30,
    38,
    46,
    54,
    62,
    70,
    78,
    86,
    94,
    102,
];

/// c_filesize 字段偏移（第 7 字段）。
pub const FIELD_FILESIZE: usize = 6;
/// c_namesize 字段偏移（第 12 字段）。
pub const FIELD_NAMESIZE: usize = 11;
/// c_check 字段偏移（第 13 字段）。
pub const FIELD_CHECK: usize = 12;

const _: () = assert!(FIELD_OFFSETS[FIELD_NAMESIZE] == 94);
const _: () = assert!(FIELD_OFFSETS[FIELD_CHECK] + 8 == HEADER_LEN);

/// 解析后的 newc 110 字节条目头（13 个 u32 整数字段，纯 Copy 小件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CpioNewcHeader {
    /// c_ino：条目 inode 号。
    pub c_ino: u32,
    /// c_mode：类型位 + 权限位。
    pub c_mode: u32,
    /// c_uid：属主 id。
    pub c_uid: u32,
    /// c_gid：属组 id。
    pub c_gid: u32,
    /// c_nlink：硬链接数。
    pub c_nlink: u32,
    /// c_mtime：修改时刻（epoch 秒）。
    pub c_mtime: u32,
    /// c_filesize：数据区字节数。
    pub c_filesize: u32,
    /// c_devmajor：主设备号。
    pub c_devmajor: u32,
    /// c_devminor：次设备号。
    pub c_devminor: u32,
    /// c_rdevmajor：rdev 主设备号。
    pub c_rdevmajor: u32,
    /// c_rdevminor：rdev 次设备号。
    pub c_rdevminor: u32,
    /// c_namesize：名区字节数（含终止 NUL，故 ≥ 1）。
    pub c_namesize: u32,
    /// c_check：校验和（newc "070701" 恒 0；newcrc "070702" 才用）。
    pub c_check: u32,
}

impl CpioNewcHeader {
    /// 整数字段数。
    pub const FIELD_COUNT: usize = 13;

    /// 从归档缓冲偏移 `at` 处解析一个 110 字节 newc 头。
    /// 短头 → HeadShort；魔数失配 → MagicMismatch；hex 坏 → HexNonHex。
    pub fn parse_at(archive: &[u8], at: usize) -> Result<CpioNewcHeader, CpioIterError> {
        if at >= archive.len() {
            return Err(CpioIterError::new(V604iCode::CursorPastEnd, 0, at as u32));
        }
        if archive.len() - at < HEADER_LEN {
            return Err(CpioIterError::new(
                V604iCode::HeadShort,
                0,
                at as u32,
            ));
        }
        let mut mi: usize = 0;
        while mi < MAGIC_LEN {
            if archive[at + mi] != NEWC_MAGIC[mi] {
                return Err(CpioIterError::new(
                    V604iCode::MagicMismatch,
                    0,
                    (at + mi) as u32,
                ));
            }
            mi += 1;
        }
        let mut fields = [0u32; 13];
        let mut fi: usize = 0;
        while fi < 13 {
            let off = at + FIELD_OFFSETS[fi];
            let value = match parse_hex8(&archive[off..off + FIELD_HEX_LEN]) {
                Ok(v) => v,
                Err(code) => return Err(CpioIterError::new(code, 0, off as u32)),
            };
            fields[fi] = value;
            fi += 1;
        }
        Ok(CpioNewcHeader {
            c_ino: fields[0],
            c_mode: fields[1],
            c_uid: fields[2],
            c_gid: fields[3],
            c_nlink: fields[4],
            c_mtime: fields[5],
            c_filesize: fields[6],
            c_devmajor: fields[7],
            c_devminor: fields[8],
            c_rdevmajor: fields[9],
            c_rdevminor: fields[10],
            c_namesize: fields[11],
            c_check: fields[12],
        })
    }

    /// 13 字段按规格序打包（重编码/指纹共用序）。
    pub const fn fields(&self) -> [u32; 13] {
        [
            self.c_ino,
            self.c_mode,
            self.c_uid,
            self.c_gid,
            self.c_nlink,
            self.c_mtime,
            self.c_filesize,
            self.c_devmajor,
            self.c_devminor,
            self.c_rdevmajor,
            self.c_rdevminor,
            self.c_namesize,
            self.c_check,
        ]
    }

    /// 文件类型闭集判别。
    pub const fn file_type(&self) -> FileType {
        FileType::from_mode(self.c_mode)
    }

    /// 权限位解码。
    pub const fn permissions(&self) -> PermissionBits {
        PermissionBits::from_mode(self.c_mode)
    }

    /// 名区文本长度 = namesize - 1（saturating，namesize=0 时为 0 并由账面红）。
    pub const fn name_text_len(&self) -> u32 {
        self.c_namesize.saturating_sub(1)
    }
}

// ==========================================================================
// 头字段账（8 要素逐条核）
// ==========================================================================

/// 8 要素审计的要素数。
pub const FIELD_AUDIT_ELEMENTS: usize = 8;

/// 8 要素序号：c_ino 登记账。
pub const AUDIT_INO: usize = 0;
/// 8 要素序号：c_mode 类型闭集 + 权限域。
pub const AUDIT_MODE: usize = 1;
/// 8 要素序号：c_uid/c_gid 16 位域。
pub const AUDIT_UIDGID: usize = 2;
/// 8 要素序号：c_nlink 合法域。
pub const AUDIT_NLINK: usize = 3;
/// 8 要素序号：c_mtime 哨兵账注。
pub const AUDIT_MTIME: usize = 4;
/// 8 要素序号：c_filesize 数据一致性。
pub const AUDIT_FILESIZE: usize = 5;
/// 8 要素序号：c_namesize 名区一致性。
pub const AUDIT_NAMESIZE: usize = 6;
/// 8 要素序号：c_check 零值核。
pub const AUDIT_CHECK: usize = 7;

/// 8 要素要素名表（与序号恒等；判据原文「ino/mode/filesize/namesize 等
/// 8 要素逐条核」的落地账面）。
pub const AUDIT_ELEMENT_NAMES: [&str; FIELD_AUDIT_ELEMENTS] = [
    "c_ino 登记账",
    "c_mode 类型闭集+权限域",
    "c_uid/c_gid 16位域",
    "c_nlink 合法域[1,0xFFFF]",
    "c_mtime 哨兵账注",
    "c_filesize 数据一致",
    "c_namesize 名区一致",
    "c_check 零值核",
];

const _: () = assert!(AUDIT_ELEMENT_NAMES.len() == FIELD_AUDIT_ELEMENTS);

/// 单条目 8 要素审计留痕（逐条布尔位图 + 通过数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldAuditReport {
    /// 审计的条目序号。
    pub entry_index: u32,
    /// 8 要素逐条通过位图（序号即 AUDIT_* 常量序）。
    pub element_pass: [bool; FIELD_AUDIT_ELEMENTS],
    /// 通过要素数（0..8）。
    pub passed_count: u8,
}

impl FieldAuditReport {
    /// 全过判定。
    pub const fn all_passed(&self) -> bool {
        self.passed_count as usize == FIELD_AUDIT_ELEMENTS
    }

    /// 首个失配要素序号（全过返回 None）。
    pub const fn first_failed(&self) -> Option<usize> {
        let mut i: usize = 0;
        while i < FIELD_AUDIT_ELEMENTS {
            if !self.element_pass[i] {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// 单条目头字段账审计输入快照（游标推进期实测的区间事实）。
pub struct FieldAuditInput {
    /// 名区文本字节数（namesize - 1 实测）。
    pub name_text_len: u32,
    /// 名区末字节是否为 NUL。
    pub nul_present: bool,
    /// 名区文本是否全为可打印 ASCII（0x20..=0x7E）。
    pub name_printable: bool,
    /// 数据区是否完整落界（data_off + filesize ≤ 归档长，游标期实测）。
    pub data_fits: bool,
    /// 该条目是否为 TRAILER（TRAILER 头 mode=0 属规格常态，mode 要素豁免）。
    pub is_trailer: bool,
}

/// 执行 8 要素逐条核，返回位图与首个失配的处置码（全过返回 None）。
pub fn audit_header(
    header: &CpioNewcHeader,
    input: &FieldAuditInput,
    entry_index: u32,
) -> (FieldAuditReport, Option<V604iCode>) {
    let mut element_pass = [false; FIELD_AUDIT_ELEMENTS];

    // 要素 0：c_ino 登记账——u32 全域解析必成，恒过；重复检测在遍历账层记注。
    element_pass[AUDIT_INO] = true;

    // 要素 1：c_mode——类型位落 S_IFMT 七类闭集 + 权限域 ≤ 0o7777。
    // TRAILER 条目规格上 mode=0（无类型位），按规格豁免。
    element_pass[AUDIT_MODE] = if input.is_trailer {
        true
    } else {
        FileType::from_mode(header.c_mode) != FileType::Unknown
            && (header.c_mode & !S_IFMT) <= MODE_PERM_MASK
    };

    // 要素 2：c_uid/c_gid——传统 16 位 id 域（见 UID_GID_MAX 来源注）。
    element_pass[AUDIT_UIDGID] = header.c_uid <= UID_GID_MAX && header.c_gid <= UID_GID_MAX;

    // 要素 3：c_nlink——[1, 0xFFFF] 合法域。
    element_pass[AUDIT_NLINK] = header.c_nlink >= 1 && header.c_nlink <= NLINK_MAX;

    // 要素 4：c_mtime——u32::MAX 为非法哨兵（生成器未初始化账注）。
    element_pass[AUDIT_MTIME] = header.c_mtime != u32::MAX;

    // 要素 5：c_filesize——数据区与归档缓冲一致性（游标期实测事实回填）。
    element_pass[AUDIT_FILESIZE] = input.data_fits;

    // 要素 6：c_namesize——≥1、≤4096、名区末字节 NUL、文本长度一致、可打印。
    element_pass[AUDIT_NAMESIZE] = header.c_namesize >= 1
        && header.c_namesize <= MAX_NAME_BYTES
        && input.nul_present
        && input.name_printable
        && input.name_text_len == header.name_text_len();

    // 要素 7：c_check——newc "070701" 恒 0。
    element_pass[AUDIT_CHECK] = header.c_check == 0;

    let mut passed_count: u8 = 0;
    let mut first_fail: Option<V604iCode> = None;
    let mut i: usize = 0;
    while i < FIELD_AUDIT_ELEMENTS {
        if element_pass[i] {
            passed_count += 1;
        } else if first_fail == None {
            first_fail = Some(match i {
                AUDIT_MODE => V604iCode::ModeTypeUnknown,
                AUDIT_NLINK => V604iCode::NlinkIllegal,
                AUDIT_FILESIZE => V604iCode::DataOverrun,
                AUDIT_NAMESIZE => V604iCode::FieldAuditMiss,
                AUDIT_CHECK => V604iCode::CheckFieldNonZero,
                _ => V604iCode::FieldAuditMiss,
            });
        }
        i += 1;
    }

    // 注：名区末字节非 NUL 已由游标推进层直判 NameNulMissing（先于本审计），
    // 故此处 AUDIT_NAMESIZE 失配统一落 FieldAuditMiss（文本域/长度账红）。

    let report = FieldAuditReport {
        entry_index,
        element_pass,
        passed_count,
    };
    (report, first_fail)
}

// ==========================================================================
// FNV-1a 32 位指纹（零依赖手写，抽样对拍基元）
// ==========================================================================

/// FNV-1a 32 位增量哈希（参数取 FNV 官方规格：offset basis 0x811C9DC5、
/// prime 0x01000193，Landon Curt Noll 公布值）。
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a32 {
    state: u32,
}

impl Fnv1a32 {
    /// FNV offset basis（官方规格值）。
    pub const OFFSET_BASIS: u32 = 0x811C_9DC5;
    /// FNV prime 32 位（官方规格值）。
    pub const PRIME: u32 = 0x0100_0193;

    /// 新 hasher。
    pub const fn new() -> Fnv1a32 {
        Fnv1a32 {
            state: Self::OFFSET_BASIS,
        }
    }

    /// 推一个字节（xor 后乘 prime，wrapping 显性）。
    pub fn push(&mut self, byte: u8) {
        self.state ^= byte as u32;
        self.state = self.state.wrapping_mul(Self::PRIME);
    }

    /// 推一段字节。
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let mut i: usize = 0;
        while i < bytes.len() {
            self.push(bytes[i]);
            i += 1;
        }
    }

    /// 推一个 u32 小端四字节（字段指纹用，序确定）。
    pub fn push_u32le(&mut self, value: u32) {
        self.push((value & 0xFF) as u8);
        self.push(((value >> 8) & 0xFF) as u8);
        self.push(((value >> 16) & 0xFF) as u8);
        self.push(((value >> 24) & 0xFF) as u8);
    }

    /// 取指纹。
    pub const fn finish(&self) -> u32 {
        self.state
    }

    /// 一步式字节序列指纹。
    pub fn digest(bytes: &[u8]) -> u32 {
        let mut h = Fnv1a32::new();
        h.push_bytes(bytes);
        h.finish()
    }
}

// ==========================================================================
// 事件账（正常事件走标签、异常事件携带段内诊断码）
// ==========================================================================

/// 事件种类标签（正常事件不占诊断码段；异常统一 Anomaly + code 定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IterEventKind {
    /// 一个 110 头解析成功。
    HeadParsed,
    /// 一个条目受理入账（含 span 与游标推进值）。
    EntryAccepted,
    /// 8 要素审计全过。
    FieldAuditPass,
    /// TRAILER!!! 终止符识别。
    TrailerSeen,
    /// 尾后残余字节账注（残余=0 合法，非零只记注）。
    ResidualNoted,
    /// 空归档显性空态。
    EmptyArchive,
    /// 字节守恒账平（Σspan=消耗 且 消耗+残余=档长）。
    SpanConserved,
    /// 遍历数与轻走推算数对平（n = n 推算数）。
    CountsMatched,
    /// 双 FNV 指纹对拍平（往返一致自证过）。
    FingerprintOk,
    /// 双跑确定性对拍平（两次独立全走序指纹恒等）。
    DeterminismOk,
    /// 异常（code 字段指向 V604iCode 段内码）。
    Anomaly,
}

impl IterEventKind {
    /// 标签内序号（事件账指纹用，与诊断段无涉）。
    pub const fn id(self) -> u16 {
        match self {
            IterEventKind::HeadParsed => 0,
            IterEventKind::EntryAccepted => 1,
            IterEventKind::FieldAuditPass => 2,
            IterEventKind::TrailerSeen => 3,
            IterEventKind::ResidualNoted => 4,
            IterEventKind::EmptyArchive => 5,
            IterEventKind::SpanConserved => 6,
            IterEventKind::CountsMatched => 7,
            IterEventKind::FingerprintOk => 8,
            IterEventKind::DeterminismOk => 9,
            IterEventKind::Anomaly => 10,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            IterEventKind::HeadParsed => "头解析",
            IterEventKind::EntryAccepted => "条目受理",
            IterEventKind::FieldAuditPass => "字段账全过",
            IterEventKind::TrailerSeen => "TRAILER 识别",
            IterEventKind::ResidualNoted => "尾后残余账注",
            IterEventKind::EmptyArchive => "空归档空态",
            IterEventKind::SpanConserved => "字节守恒账平",
            IterEventKind::CountsMatched => "计数对平",
            IterEventKind::FingerprintOk => "指纹对拍平",
            IterEventKind::DeterminismOk => "双跑确定性平",
            IterEventKind::Anomaly => "异常",
        }
    }
}

/// 单条事件（Copy 计数件 + String 注）。
#[derive(Clone, Debug)]
pub struct IterEvent {
    /// 事件种类。
    pub kind: IterEventKind,
    /// 关联条目序号（无关联填 u32::MAX）。
    pub entry_index: u32,
    /// 关联归档内偏移（无关联填 u32::MAX）。
    pub offset: u32,
    /// 段内诊断码（仅 Anomaly 携带；正常事件为 None）。
    pub code: Option<u16>,
    /// 事件注（中文）。
    pub note: String,
}

/// 事件账（append-only + 增量指纹；指纹对每事件按 标签序+条目+偏移+码 序入哈希）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    events: Vec<IterEvent>,
    fingerprint: u32,
    anomaly_count: u32,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            events: Vec::new(),
            fingerprint: Fnv1a32::OFFSET_BASIS,
            anomaly_count: 0,
        }
    }

    /// 事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否无事件。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 账指纹（增量 FNV，双跑确定性对拍基元）。
    pub const fn fingerprint(&self) -> u32 {
        self.fingerprint
    }

    /// 异常事件数。
    pub const fn anomaly_count(&self) -> u32 {
        self.anomaly_count
    }

    /// 追加事件（增量指纹同步推进）。
    pub fn push(&mut self, event: IterEvent) {
        let mut h = Fnv1a32 { state: self.fingerprint };
        h.push_u32le(event.kind.id() as u32);
        h.push_u32le(event.entry_index);
        h.push_u32le(event.offset);
        match event.code {
            Some(c) => {
                h.push_u32le(1);
                h.push_u32le(c as u32);
                self.anomaly_count = self.anomaly_count.saturating_add(1);
            }
            None => h.push_u32le(0),
        }
        self.fingerprint = h.finish();
        self.events.push(event);
    }

    /// 追加简注正常事件的便捷入口。
    pub fn push_note(&mut self, kind: IterEventKind, entry_index: u32, offset: u32, note: &str) {
        self.push(IterEvent {
            kind,
            entry_index,
            offset,
            code: None,
            note: String::from(note),
        });
    }

    /// 追加异常事件的便捷入口。
    pub fn push_anomaly(&mut self, code: V604iCode, entry_index: u32, offset: u32, note: &str) {
        self.push(IterEvent {
            kind: IterEventKind::Anomaly,
            entry_index,
            offset,
            code: Some(code.code()),
            note: String::from(note),
        });
    }

    /// 事件切片（只读视图）。
    pub fn events(&self) -> &[IterEvent] {
        &self.events
    }

    /// 健康面（异常数与首个异常定位）。
    pub fn health(&self) -> IterHealth {
        let mut first_code: Option<u16> = None;
        let mut first_detail: &'static str = "";
        let mut first_entry: u32 = u32::MAX;
        let mut first_offset: u32 = u32::MAX;
        let mut i: usize = 0;
        while i < self.events.len() {
            let event = &self.events[i];
            if event.kind == IterEventKind::Anomaly {
                if first_code == None {
                    if let Some(c) = event.code {
                        first_code = Some(c);
                        first_detail = match V604iCode::from_code(c) {
                            Some(cc) => cc.detail(),
                            None => "段外码（账面污染，须查事件账写入口）",
                        };
                    }
                    first_entry = event.entry_index;
                    first_offset = event.offset;
                }
            }
            i += 1;
        }
        IterHealth {
            ok: self.anomaly_count == 0,
            anomaly_count: self.anomaly_count,
            first_anomaly_code: first_code,
            first_anomaly_detail: first_detail,
            first_anomaly_entry: first_entry,
            first_anomaly_offset: first_offset,
            events: self.events.len() as u32,
            ledger_fingerprint: self.fingerprint,
        }
    }
}

// ==========================================================================
// 健康面
// ==========================================================================

/// 遍历域健康快照（ok = 零异常事件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IterHealth {
    /// 总判：零异常。
    pub ok: bool,
    /// 异常事件数。
    pub anomaly_count: u32,
    /// 首个异常段内码。
    pub first_anomaly_code: Option<u16>,
    /// 首个异常中文可操作诊断。
    pub first_anomaly_detail: &'static str,
    /// 首个异常条目序号。
    pub first_anomaly_entry: u32,
    /// 首个异常偏移。
    pub first_anomaly_offset: u32,
    /// 事件总数。
    pub events: u32,
    /// 事件账指纹。
    pub ledger_fingerprint: u32,
}

// ==========================================================================
// 条目（受理产物）
// ==========================================================================

/// 受理完成的条目（头字段 + 名 + 数据区窗口事实 + 8 要素审计留痕）。
#[derive(Clone, Debug)]
pub struct CpioEntry {
    /// 条目序号（0 起，连续）。
    pub index: u32,
    /// 110 头解析件。
    pub header: CpioNewcHeader,
    /// 名区文本（已剥 NUL）。
    pub name: String,
    /// 头在归档内的偏移。
    pub header_offset: u32,
    /// 名区起点偏移（= header_offset + 110）。
    pub name_offset: u32,
    /// 数据区起点偏移（4 对齐收口后）。
    pub data_offset: u32,
    /// 数据区字节数（= c_filesize）。
    pub data_len: u32,
    /// 名区垫字节数（0..3）。
    pub name_pad: u8,
    /// 数据区垫字节数（0..3）。
    pub data_pad: u8,
    /// 是否 TRAILER 终止符条目。
    pub is_trailer: bool,
    /// 8 要素审计留痕。
    pub audit: FieldAuditReport,
}

impl CpioEntry {
    /// 条目总 span（u64 显性，供守恒账）：
    /// span = 110 头 + namesize + 名垫 + filesize + 数据垫。
    pub const fn total_span_u64(&self) -> u64 {
        HEADER_LEN as u64
            + self.header.c_namesize as u64
            + self.name_pad as u64
            + self.header.c_filesize as u64
            + self.data_pad as u64
    }

    /// 数据区只读窗口（越界 None——窗口原语红线）。
    pub fn data_window<'a>(&self, archive: &'a [u8]) -> Option<&'a [u8]> {
        let start = self.data_offset as usize;
        let end = start.checked_add(self.data_len as usize)?;
        archive.get(start..end)
    }

    /// 文件类型闭集判别。
    pub const fn file_type(&self) -> FileType {
        self.header.file_type()
    }
}

// ==========================================================================
// CpioIterator 安全遍历（游标推进 + 遍历账 + 边界防线）
// ==========================================================================

/// 安全遍历器（持归档副本，游标推进真实实现，逐条入事件账）。
///
/// 推进式：`下一条目头偏移 = 本头偏移 + 110 + pad4(名区) + pad4(数据区)`，
/// 名区/数据区垫按归档起始 4 字节边界收口（newc 规格，Linux initramfs unpack
/// 同式）。所有推进走 checked/saturating，绝不静默回绕。
#[derive(Clone, Debug)]
pub struct CpioIterator {
    archive: Vec<u8>,
    cursor: usize,
    next_index: u32,
    consumed_bytes: u64,
    trailer_seen: bool,
    finished: bool,
    empty_announced: bool,
    last_ino: u32,
    ino_duplicates: u32,
    ledger: EventLedger,
}

impl CpioIterator {
    /// 持有归档副本构造（遍历器独占该副本，外部零共享）。
    pub fn from_vec(archive: Vec<u8>) -> CpioIterator {
        CpioIterator {
            archive,
            cursor: 0,
            next_index: 0,
            consumed_bytes: 0,
            trailer_seen: false,
            finished: false,
            empty_announced: false,
            last_ino: 0,
            ino_duplicates: 0,
            ledger: EventLedger::new(),
        }
    }

    /// 从切片拷贝构造。
    pub fn from_slice(archive: &[u8]) -> CpioIterator {
        CpioIterator::from_vec(Vec::from(archive))
    }

    /// 当前游标（归档内字节偏移）。
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// 下一待受理条目序号。
    pub const fn next_index(&self) -> u32 {
        self.next_index
    }

    /// 已受理条目累计消耗字节。
    pub const fn consumed_bytes(&self) -> u64 {
        self.consumed_bytes
    }

    /// TRAILER 是否已识别。
    pub const fn trailer_seen(&self) -> bool {
        self.trailer_seen
    }

    /// 归档是否为空（显性空态判定基）。
    pub fn is_empty_archive(&self) -> bool {
        self.archive.is_empty()
    }

    /// 事件账只读视图。
    pub fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 取走事件账（遍历收尾用）。
    pub fn into_ledger(self) -> EventLedger {
        self.ledger
    }

    /// 健康面（零异常事件 = ok）。
    pub fn health(&self) -> IterHealth {
        self.ledger.health()
    }

    /// c_ino 重复登记计数（重复不拒收，账注留痕——判据「账注」语义）。
    pub const fn ino_duplicates(&self) -> u32 {
        self.ino_duplicates
    }

    /// 受理下一条目；归档尽/收档后返回 Ok(None)。
    ///
    /// 错误闭集：HeadShort/MagicMismatch/HexNonHex/NamesizeZero/NameNulMissing/
    /// DataOverrun/CursorPastEnd/PadNonZero/LedgerMiss/FieldAuditMiss/
    /// CheckFieldNonZero/ReentryGuard（8 要素账红映射见 audit_header）。
    pub fn next_entry(&mut self) -> Result<Option<CpioEntry>, CpioIterError> {
        // 空归档显性空态：首次询问即宣布 EmptyArchive，非错误。
        if !self.empty_announced {
            self.empty_announced = true;
            if self.archive.is_empty() {
                self.finished = true;
                self.ledger
                    .push_note(IterEventKind::EmptyArchive, u32::MAX, 0, "空归档：0 字节，显性空态");
                return Ok(None);
            }
        }
        if self.finished {
            return Ok(None);
        }

        // 边界防线一：残余不足一个完整头且游标未在末尾 → 游标越过末尾。
        let remaining = self.archive.len().saturating_sub(self.cursor);
        if remaining == 0 {
            // 无 TRAILER 而自然耗尽：由 traverse_archive 层终判 TrailerMissing。
            self.finished = true;
            return Ok(None);
        }
        if remaining < HEADER_LEN {
            // 尚未受理任何条目即不足一头 → 短头；已推进后残头 → 游标越过末尾。
            let code = if self.next_index == 0 {
                V604iCode::HeadShort
            } else {
                V604iCode::CursorPastEnd
            };
            self.ledger.push_anomaly(
                code,
                self.next_index,
                self.cursor as u32,
                "残余不足 110 字节头",
            );
            return Err(CpioIterError::new(code, self.next_index, self.cursor as u32));
        }

        // 110 头真实解析（短头/魔数/hex 三道防线内嵌；错误先入异常账再上抛）。
        let header_offset = self.cursor;
        let header = match CpioNewcHeader::parse_at(&self.archive, header_offset) {
            Ok(h) => h,
            Err(err) => {
                self.ledger.push_anomaly(
                    err.code(),
                    self.next_index,
                    err.offset(),
                    "110 字节头解析失败",
                );
                return Err(CpioIterError::new(err.code(), self.next_index, err.offset()));
            }
        };
        self.ledger.push_note(
            IterEventKind::HeadParsed,
            self.next_index,
            header_offset as u32,
            "110 字节头解析成功",
        );

        // namesize = 0 拒收（newc 名区至少含 NUL 一字节）。
        if header.c_namesize == 0 {
            self.ledger.push_anomaly(
                V604iCode::NamesizeZero,
                self.next_index,
                (header_offset + FIELD_OFFSETS[FIELD_NAMESIZE]) as u32,
                "c_namesize = 0",
            );
            return Err(CpioIterError::new(
                V604iCode::NamesizeZero,
                self.next_index,
                (header_offset + FIELD_OFFSETS[FIELD_NAMESIZE]) as u32,
            ));
        }

        // 名区窗口：namesize 字节，须完整落界（边界防线二：越出缓冲拒收）。
        let name_start = header_offset + HEADER_LEN;
        let name_end = match name_start.checked_add(header.c_namesize as usize) {
            Some(e) => e,
            None => {
                return Err(CpioIterError::new(
                    V604iCode::DataOverrun,
                    self.next_index,
                    name_start as u32,
                ));
            }
        };
        if name_end > self.archive.len() {
            self.ledger.push_anomaly(
                V604iCode::DataOverrun,
                self.next_index,
                name_start as u32,
                "名区越出归档缓冲",
            );
            return Err(CpioIterError::new(
                V604iCode::DataOverrun,
                self.next_index,
                name_start as u32,
            ));
        }

        // 名区末字节必须为 NUL（名字不含 NUL 终止拒收）。
        if self.archive[name_end - 1] != NAME_TERMINATOR {
            self.ledger.push_anomaly(
                V604iCode::NameNulMissing,
                self.next_index,
                (name_end - 1) as u32,
                "名区末字节非 NUL",
            );
            return Err(CpioIterError::new(
                V604iCode::NameNulMissing,
                self.next_index,
                (name_end - 1) as u32,
            ));
        }

        // 名文本可打印域核（0x20..=0x7E；路径控制字符/内嵌 NUL 记失配）。
        let text_len = (header.c_namesize - 1) as usize;
        let mut name_printable = true;
        let mut ti: usize = 0;
        while ti < text_len {
            let b = self.archive[name_start + ti];
            if !(0x20..=0x7E).contains(&b) {
                name_printable = false;
            }
            ti += 1;
        }
        let name_bytes: Vec<u8> = self.archive[name_start..name_start + text_len].to_vec();
        let name = match String::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => String::new(), // 可打印域已核，此分支纯防守（不可达即审计红）。
        };

        // TRAILER!!! 终止符识别：收档 + 尾后残余账注。
        let is_trailer = name == TRAILER_NAME;
        if is_trailer {
            let name_pad = pad_needed((header_offset + HEADER_LEN) as u32 + header.c_namesize);
            let data_start = align_up(name_start + header.c_namesize as usize);
            let data_pad = pad_needed(header.c_filesize);
            let trailer_end = match data_start
                .checked_add(header.c_filesize as usize)
                .and_then(|v| v.checked_add(data_pad as usize))
            {
                Some(e) => e,
                None => {
                    return Err(CpioIterError::new(
                        V604iCode::DataOverrun,
                        self.next_index,
                        data_start as u32,
                    ));
                }
            };
            if trailer_end > self.archive.len() {
                self.ledger.push_anomaly(
                    V604iCode::DataOverrun,
                    self.next_index,
                    data_start as u32,
                    "TRAILER 条目 span 越出归档缓冲",
                );
                return Err(CpioIterError::new(
                    V604iCode::DataOverrun,
                    self.next_index,
                    data_start as u32,
                ));
            }
            let span = (trailer_end - header_offset) as u64;
            self.consumed_bytes = self.consumed_bytes.saturating_add(span);
            let residual = (self.archive.len() - trailer_end) as u64;
            self.ledger.push_note(
                IterEventKind::EntryAccepted,
                self.next_index,
                header_offset as u32,
                "TRAILER 条目受理（收档）",
            );
            self.ledger.push_note(
                IterEventKind::TrailerSeen,
                self.next_index,
                header_offset as u32,
                "TRAILER!!! 终止符识别",
            );
            self.ledger.push_note(
                IterEventKind::ResidualNoted,
                self.next_index,
                trailer_end as u32,
                "尾后残余字节账注（残余=0 合法）",
            );
            self.cursor = trailer_end;
            self.finished = true;
            self.trailer_seen = true;
            let _ = (name_pad, data_pad, residual);
            return Ok(None);
        }

        // 对齐推进：数据区起点 = align4(头偏移 + 110 + namesize)（归档起始界）。
        let data_start = align_up(name_start + header.c_namesize as usize);
        let data_end = match data_start.checked_add(header.c_filesize as usize) {
            Some(e) => e,
            None => {
                return Err(CpioIterError::new(
                    V604iCode::DataOverrun,
                    self.next_index,
                    data_start as u32,
                ));
            }
        };
        if data_end > self.archive.len() {
            self.ledger.push_anomaly(
                V604iCode::DataOverrun,
                self.next_index,
                data_start as u32,
                "数据区越出归档缓冲",
            );
            return Err(CpioIterError::new(
                V604iCode::DataOverrun,
                self.next_index,
                data_start as u32,
            ));
        }
        let name_pad = pad_needed((header_offset + HEADER_LEN) as u32 + header.c_namesize);
        let data_pad = pad_needed(header.c_filesize);

        // 对齐垫字节必须为 0（canonical newc；对齐错注入检出点）。
        let name_pad_ok = pad_bytes_zero(&self.archive, name_end, name_pad as usize);
        let data_pad_ok = pad_bytes_zero(&self.archive, data_end, data_pad as usize);
        if name_pad_ok != Some(true) || data_pad_ok != Some(true) {
            let pad_off = if name_pad_ok != Some(true) {
                name_end
            } else {
                data_end
            };
            self.ledger.push_anomaly(
                V604iCode::PadNonZero,
                self.next_index,
                pad_off as u32,
                "对齐垫字节非零",
            );
            return Err(CpioIterError::new(
                V604iCode::PadNonZero,
                self.next_index,
                pad_off as u32,
            ));
        }

        // 8 要素头字段账逐条核（逐条留痕；失配即红）。
        let data_fits = data_end <= self.archive.len();
        let audit_input = FieldAuditInput {
            name_text_len: text_len as u32,
            nul_present: true,
            name_printable,
            data_fits,
            is_trailer: false,
        };
        let (audit, audit_fail) = audit_header(&header, &audit_input, self.next_index);
        if let Some(code) = audit_fail {
            self.ledger.push_anomaly(code, self.next_index, header_offset as u32, "8 要素账失配");
            return Err(CpioIterError::new(code, self.next_index, header_offset as u32));
        }

        // c_ino 重复登记账注（重复不拒收，重复数入账）。
        if self.next_index > 0 && header.c_ino == self.last_ino {
            self.ino_duplicates = self.ino_duplicates.saturating_add(1);
            self.ledger.push_note(
                IterEventKind::ResidualNoted,
                self.next_index,
                header_offset as u32,
                "c_ino 与前条目重复（账注，不拒收）",
            );
        }
        self.last_ino = header.c_ino;

        // 遍历账：span 守恒（Σspan = 游标推进）+ 序号连续。
        let entry = CpioEntry {
            index: self.next_index,
            header,
            name,
            header_offset: header_offset as u32,
            name_offset: name_start as u32,
            data_offset: data_start as u32,
            data_len: header.c_filesize,
            name_pad: name_pad as u8,
            data_pad: data_pad as u8,
            is_trailer: false,
            audit,
        };
        let span = entry.total_span_u64();
        let next_cursor = match (self.cursor as u64).checked_add(span) {
            Some(v) => v,
            None => {
                return Err(CpioIterError::new(
                    V604iCode::LedgerMiss,
                    self.next_index,
                    self.cursor as u32,
                ));
            }
        };
        if next_cursor > self.archive.len() as u64 {
            self.ledger.push_anomaly(
                V604iCode::LedgerMiss,
                self.next_index,
                self.cursor as u32,
                "span 推进越界（账失守）",
            );
            return Err(CpioIterError::new(
                V604iCode::LedgerMiss,
                self.next_index,
                self.cursor as u32,
            ));
        }
        if self.consumed_bytes.saturating_add(span) != next_cursor {
            self.ledger.push_anomaly(
                V604iCode::LedgerMiss,
                self.next_index,
                self.cursor as u32,
                "消耗累计与游标不守恒",
            );
            return Err(CpioIterError::new(
                V604iCode::LedgerMiss,
                self.next_index,
                self.cursor as u32,
            ));
        }
        // 状态机守卫：游标必须严格单调推进（防回环）。
        if next_cursor <= self.cursor as u64 && span == 0 {
            self.ledger.push_anomaly(
                V604iCode::ReentryGuard,
                self.next_index,
                self.cursor as u32,
                "游标非单调推进",
            );
            return Err(CpioIterError::new(
                V604iCode::ReentryGuard,
                self.next_index,
                self.cursor as u32,
            ));
        }

        self.consumed_bytes = next_cursor;
        self.ledger.push_note(
            IterEventKind::EntryAccepted,
            self.next_index,
            header_offset as u32,
            "条目受理（span/游标入账）",
        );
        self.ledger.push_note(
            IterEventKind::FieldAuditPass,
            self.next_index,
            header_offset as u32,
            "8 要素审计全过",
        );
        self.cursor = next_cursor as usize;
        self.next_index = self.next_index.saturating_add(1);
        Ok(Some(entry))
    }
}

// ==========================================================================
// 全走引擎（审计走 + 轻走推算 + 重建对拍 + 双跑确定性）
// ==========================================================================

/// 重建用条目档案（名 + 头字段 + 数据副本，重编码输入）。
#[derive(Clone, Debug)]
pub struct CpioEntryRecord {
    /// 110 头解析件。
    pub header: CpioNewcHeader,
    /// 名区文本（已剥 NUL）。
    pub name: String,
    /// 数据区副本。
    pub data: Vec<u8>,
}

/// 审计全走产物（遍历账 + 事件账 + 序指纹 + 重建件）。
#[derive(Clone, Debug)]
pub struct WalkOutcome {
    /// 受理条目（含审计留痕与窗口事实）。
    pub entries: Vec<CpioEntry>,
    /// 重建用档案（数据已拷出）。
    pub records: Vec<CpioEntryRecord>,
    /// 事件账。
    pub ledger: EventLedger,
    /// 受理累计消耗字节（Σspan）。
    pub consumed: u64,
    /// TRAILER 后残余字节。
    pub residual: u64,
    /// TRAILER 是否识别。
    pub trailer_seen: bool,
    /// TRAILER 条目序号（即受理条目数，因 trailer 是第 n+1 个头）。
    pub trailer_entry_index: Option<u32>,
    /// TRAILER 条目 span（110+名垫+0+0；守恒账的收档半边）。
    pub trailer_span: u64,
    /// 空归档显性空态。
    pub empty_archive: bool,
    /// 条目序指纹（名+13字段+数据 按序 FNV——双跑确定性对拍基元）。
    pub sequence_fingerprint: u32,
}

/// 审计全走（与 CpioIterator 同一推进引擎，产出全量账）。
pub fn static_walk(archive: &[u8]) -> Result<WalkOutcome, CpioIterError> {
    let mut iter = CpioIterator::from_slice(archive);
    let mut entries: Vec<CpioEntry> = Vec::new();
    let mut records: Vec<CpioEntryRecord> = Vec::new();
    let mut seq_hash = Fnv1a32::new();
    let empty_archive = archive.is_empty();
    let mut entry_span_sum: u64 = 0;
    let mut pre_cursor: usize;
    loop {
        // 每次询问前的游标即下一头偏移（TRAILER 记录补录定位用）。
        pre_cursor = iter.cursor();
        let entry = match iter.next_entry() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(err) => return Err(err),
        };
        // 序指纹：条目序号 + 名 + 13 字段（小端序）+ 数据 依序入哈希。
        seq_hash.push_u32le(entry.index);
        seq_hash.push_bytes(entry.name.as_bytes());
        seq_hash.push(0);
        let fields = entry.header.fields();
        let mut fi: usize = 0;
        while fi < fields.len() {
            seq_hash.push_u32le(fields[fi]);
            fi += 1;
        }
        if let Some(data) = entry.data_window(archive) {
            seq_hash.push_bytes(data);
        }
        entry_span_sum = entry_span_sum.saturating_add(entry.total_span_u64());
        records.push(CpioEntryRecord {
            header: entry.header,
            name: entry.name.clone(),
            data: match entry.data_window(archive) {
                Some(d) => Vec::from(d),
                None => Vec::new(),
            },
        });
        entries.push(entry);
    }
    let trailer_seen = iter.trailer_seen();
    let trailer_entry_index = if trailer_seen {
        Some(iter.next_index())
    } else {
        None
    };
    let consumed = iter.consumed_bytes();
    // TRAILER 记录补录：重建字节流必须含终止符条目（往返一致自证的前提）。
    if trailer_seen {
        if let Ok(trailer_header) = CpioNewcHeader::parse_at(archive, pre_cursor) {
            records.push(CpioEntryRecord {
                header: trailer_header,
                name: String::from(TRAILER_NAME),
                data: Vec::new(),
            });
        }
    }
    let trailer_span = consumed.saturating_sub(entry_span_sum);
    Ok(WalkOutcome {
        consumed,
        residual: (archive.len() as u64).saturating_sub(consumed),
        entries,
        records,
        ledger: iter.into_ledger(),
        trailer_seen,
        trailer_entry_index,
        trailer_span,
        empty_archive,
        sequence_fingerprint: seq_hash.finish(),
    })
}

/// canonical 重编码：条目档案 → newc 字节流（魔数 + 13×hex8 + 名+NUL+垫 +
/// 数据+垫，垫写 0）。往返一致自证的重建半边。
pub fn rebuild_archive(records: &[CpioEntryRecord]) -> Result<Vec<u8>, CpioIterError> {
    let mut out: Vec<u8> = Vec::new();
    let mut ri: usize = 0;
    while ri < records.len() {
        let record = &records[ri];
        // 账面一致：namesize 必须等于 名文本 + 1（NUL）。
        let expect_namesize = record.name.len() as u32 + 1;
        if record.header.c_namesize != expect_namesize {
            return Err(CpioIterError::new(
                V604iCode::FingerprintMiss,
                ri as u32,
                0,
            ));
        }
        // 数据长度必须与 c_filesize 一致。
        if record.data.len() as u32 != record.header.c_filesize {
            return Err(CpioIterError::new(
                V604iCode::FingerprintMiss,
                ri as u32,
                0,
            ));
        }
        out.extend_from_slice(&NEWC_MAGIC);
        let fields = record.header.fields();
        let mut fi: usize = 0;
        while fi < fields.len() {
            let enc = encode_hex8(fields[fi]);
            out.extend_from_slice(&enc);
            fi += 1;
        }
        out.extend_from_slice(record.name.as_bytes());
        out.push(NAME_TERMINATOR);
        let name_pad = pad_needed(HEADER_LEN as u32 + record.header.c_namesize);
        let mut pi: u32 = 0;
        while pi < name_pad {
            out.push(0);
            pi += 1;
        }
        out.extend_from_slice(&record.data);
        let data_pad = pad_needed(record.header.c_filesize);
        let mut di: u32 = 0;
        while di < data_pad {
            out.push(0);
            di += 1;
        }
        ri += 1;
    }
    Ok(out)
}

/// 轻走推算数（独立第二走：不建名、不审计、不拷数据，只读魔数与
/// namesize/filesize 两字段推进计数）——判据「n 条目 = n 推算数」的推算半边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightCount {
    /// 推算条目数（不含 TRAILER）。
    pub entries: u32,
    /// 是否走到 TRAILER。
    pub reached_trailer: bool,
    /// 轻走消耗字节。
    pub consumed: u64,
    /// 停走码（正常走尽为 None）。
    pub stop_code: Option<u16>,
}

/// 轻走计数器（与审计走完全独立的最小解析推进）。
pub fn count_entries_light(archive: &[u8]) -> LightCount {
    let mut count: u32 = 0;
    let mut pos: usize = 0;
    let total = archive.len();
    loop {
        let remaining = total.saturating_sub(pos);
        if remaining == 0 {
            return LightCount {
                entries: count,
                reached_trailer: false,
                consumed: pos as u64,
                stop_code: None,
            };
        }
        if remaining < HEADER_LEN {
            return LightCount {
                entries: count,
                reached_trailer: false,
                consumed: pos as u64,
                stop_code: Some(V604iCode::CursorPastEnd.code()),
            };
        }
        // 魔数快核。
        let mut mi: usize = 0;
        let mut magic_ok = true;
        while mi < MAGIC_LEN {
            if archive[pos + mi] != NEWC_MAGIC[mi] {
                magic_ok = false;
            }
            mi += 1;
        }
        if !magic_ok {
            return LightCount {
                entries: count,
                reached_trailer: false,
                consumed: pos as u64,
                stop_code: Some(V604iCode::MagicMismatch.code()),
            };
        }
        // 只取 namesize 与 filesize 两个字段。
        let namesize = match parse_hex8(&archive[pos + FIELD_OFFSETS[FIELD_NAMESIZE]
            ..pos + FIELD_OFFSETS[FIELD_NAMESIZE] + FIELD_HEX_LEN])
        {
            Ok(v) => v,
            Err(_) => {
                return LightCount {
                    entries: count,
                    reached_trailer: false,
                    consumed: pos as u64,
                    stop_code: Some(V604iCode::HexNonHex.code()),
                };
            }
        };
        let filesize = match parse_hex8(&archive[pos + FIELD_OFFSETS[FIELD_FILESIZE]
            ..pos + FIELD_OFFSETS[FIELD_FILESIZE] + FIELD_HEX_LEN])
        {
            Ok(v) => v,
            Err(_) => {
                return LightCount {
                    entries: count,
                    reached_trailer: false,
                    consumed: pos as u64,
                    stop_code: Some(V604iCode::HexNonHex.code()),
                };
            }
        };
        if namesize == 0 || namesize as usize > remaining {
            return LightCount {
                entries: count,
                reached_trailer: false,
                consumed: pos as u64,
                stop_code: Some(V604iCode::DataOverrun.code()),
            };
        }
        let name_start = pos + HEADER_LEN;
        let data_start = align_up(name_start + namesize as usize);
        let data_pad = pad_needed(filesize);
        let span_end = match data_start
            .checked_add(filesize as usize)
            .and_then(|v| v.checked_add(data_pad as usize))
        {
            Some(e) => e,
            None => {
                return LightCount {
                    entries: count,
                    reached_trailer: false,
                    consumed: pos as u64,
                    stop_code: Some(V604iCode::DataOverrun.code()),
                };
            }
        };
        if span_end > total {
            return LightCount {
                entries: count,
                reached_trailer: false,
                consumed: pos as u64,
                stop_code: Some(V604iCode::DataOverrun.code()),
            };
        }
        // 名快读：判 TRAILER。
        let text_len = (namesize - 1) as usize;
        let is_trailer = text_len == TRAILER_NAME.len()
            && &archive[name_start..name_start + text_len] == TRAILER_NAME.as_bytes();
        if is_trailer {
            return LightCount {
                entries: count,
                reached_trailer: true,
                consumed: span_end as u64,
                stop_code: None,
            };
        }
        count = count.saturating_add(1);
        pos = span_end;
    }
}

// ==========================================================================
// 归档形态概括
// ==========================================================================

/// 归档形态概括（类型直方图 + 名界 + 数据总量，账面一屏）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveShape {
    /// 受理条目数（不含 TRAILER）。
    pub n_entries: u32,
    /// 七类闭集直方图（下标 = FileType::id，0..6；Unknown 不入图另记）。
    pub type_histogram: [u32; FileType::KNOWN_COUNT],
    /// Unknown 类型条目数（审计层恒拒收，此处为独立观察位）。
    pub unknown_types: u32,
    /// 最长名文本（0 表示无条目）。
    pub max_name_len: u32,
    /// 最短名文本（u32::MAX 表示无条目）。
    pub min_name_len: u32,
    /// Σ 数据字节数。
    pub total_data_bytes: u64,
    /// Σ 条目 span 字节数（含 TRAILER span）。
    pub total_span_bytes: u64,
}

impl ArchiveShape {
    /// 从全走产物概括。
    pub fn from_walk(outcome: &WalkOutcome) -> ArchiveShape {
        let mut histogram = [0u32; FileType::KNOWN_COUNT];
        let mut unknown: u32 = 0;
        let mut max_name: u32 = 0;
        let mut min_name: u32 = u32::MAX;
        let mut total_data: u64 = 0;
        let mut total_span: u64 = 0;
        let mut i: usize = 0;
        while i < outcome.entries.len() {
            let entry = &outcome.entries[i];
            match entry.file_type() {
                FileType::Unknown => unknown = unknown.saturating_add(1),
                ft => {
                    let slot = ft.id();
                    histogram[slot] = histogram[slot].saturating_add(1);
                }
            }
            let name_len = entry.name.len() as u32;
            if name_len > max_name {
                max_name = name_len;
            }
            if name_len < min_name {
                min_name = name_len;
            }
            total_data = total_data.saturating_add(entry.data_len as u64);
            total_span = total_span.saturating_add(entry.total_span_u64());
            i += 1;
        }
        if outcome.entries.is_empty() {
            min_name = 0;
        }
        ArchiveShape {
            n_entries: outcome.entries.len() as u32,
            type_histogram: histogram,
            unknown_types: unknown,
            max_name_len: max_name,
            min_name_len: min_name,
            total_data_bytes: total_data,
            total_span_bytes: total_span,
        }
    }
}

// ==========================================================================
// traverse_archive 总判入口（遍历账 + 字段账 + TRAILER + 对拍 + 确定性）
// ==========================================================================

/// 遍历总判报告（判据三账面：遍历账齐 / 头字段账逐条核 / TRAILER+残余账注）。
#[derive(Clone, Debug)]
pub struct TraverseReport {
    /// 空归档显性空态。
    pub empty_archive: bool,
    /// 受理条目数 n。
    pub n_entries: u32,
    /// 轻走推算数 n'。
    pub n_predicted: u32,
    /// n = n' 判定。
    pub counts_match: bool,
    /// 受理消耗字节。
    pub consumed_bytes: u64,
    /// TRAILER 后残余字节（0 合法）。
    pub residual_bytes: u64,
    /// 字节守恒账平（Σspan=消耗 且 消耗+残余=档长）。
    pub span_conserved: bool,
    /// TRAILER 是否识别。
    pub trailer_seen: bool,
    /// TRAILER 条目序号。
    pub trailer_entry_index: Option<u32>,
    /// 8 要素审计全过条目数。
    pub audit_passed_entries: u32,
    /// 各要素全档通过计数（8 槽，跨条目累计）。
    pub audit_element_pass_counts: [u32; FIELD_AUDIT_ELEMENTS],
    /// 原始字节 FNV 指纹。
    pub fingerprint_raw: u32,
    /// 重建字节流 FNV 指纹。
    pub fingerprint_rebuilt: u32,
    /// 双指纹对拍平（往返一致自证）。
    pub fingerprint_match: bool,
    /// 双跑确定性平（两次独立全走序指纹恒等）。
    pub deterministic: bool,
    /// 条目序指纹。
    pub sequence_fingerprint: u32,
    /// 归档形态概括。
    pub shape: ArchiveShape,
    /// 事件账。
    pub ledger: EventLedger,
    /// 健康面。
    pub health: IterHealth,
}

/// 总判入口：一次进档，出全账。任何审计红即 Err 短路；空归档走显性空态。
pub fn traverse_archive(archive: &[u8]) -> Result<TraverseReport, CpioIterError> {
    // 第一走（审计全走）。
    let walk1 = static_walk(archive)?;

    // 空归档：显性空态直出（0 条目 = 0 推算数，账齐；无 TRAILER 不判失守）。
    if archive.is_empty() {
        let shape = ArchiveShape::from_walk(&walk1);
        let mut ledger = walk1.ledger;
        ledger.push_note(
            IterEventKind::CountsMatched,
            u32::MAX,
            0,
            "空归档：n=0 = n推算=0，账齐",
        );
        let health = ledger.health();
        return Ok(TraverseReport {
            empty_archive: true,
            n_entries: 0,
            n_predicted: 0,
            counts_match: true,
            consumed_bytes: 0,
            residual_bytes: 0,
            span_conserved: true,
            trailer_seen: false,
            trailer_entry_index: None,
            audit_passed_entries: 0,
            audit_element_pass_counts: [0; FIELD_AUDIT_ELEMENTS],
            fingerprint_raw: Fnv1a32::digest(archive),
            fingerprint_rebuilt: Fnv1a32::OFFSET_BASIS,
            fingerprint_match: true,
            deterministic: true,
            sequence_fingerprint: walk1.sequence_fingerprint,
            shape,
            ledger,
            health,
        });
    }

    // 轻走推算（独立第二账）。
    let light = count_entries_light(archive);
    let n_entries = walk1.entries.len() as u32;
    let counts_match = n_entries == light.entries && walk1.trailer_seen == light.reached_trailer;

    // 字节守恒账：Σspan = 消耗 且 消耗 + 残余 = 档长。
    let mut span_sum: u64 = 0;
    let mut i: usize = 0;
    while i < walk1.entries.len() {
        span_sum = span_sum.saturating_add(walk1.entries[i].total_span_u64());
        i += 1;
    }
    let span_conserved = span_sum.saturating_add(walk1.trailer_span) == walk1.consumed
        && walk1.consumed.saturating_add(walk1.residual) == archive.len() as u64;

    // TRAILER 尾核（非空归档必须以 TRAILER!!! 收档）。
    if !walk1.trailer_seen {
        return Err(CpioIterError::new(
            V604iCode::TrailerMissing,
            n_entries,
            archive.len() as u32,
        ));
    }

    // 计数账红：n ≠ n' 即账失守。
    if !counts_match {
        return Err(CpioIterError::new(
            V604iCode::LedgerMiss,
            n_entries,
            0,
        ));
    }

    // 守恒账红。
    if !span_conserved {
        return Err(CpioIterError::new(
            V604iCode::LedgerMiss,
            n_entries,
            walk1.consumed as u32,
        ));
    }

    // 双 FNV 指纹对拍：原始字节 vs 重建字节流（往返一致自证）。
    // 尾后残余（合法账注件）按原样并入重建流，保证档长级逐字节对拍。
    let fingerprint_raw = Fnv1a32::digest(archive);
    let mut rebuilt = rebuild_archive(&walk1.records)?;
    if walk1.residual > 0 && (walk1.residual as usize) <= archive.len() {
        let residue_start = archive.len() - walk1.residual as usize;
        rebuilt.extend_from_slice(&archive[residue_start..]);
    }
    let fingerprint_rebuilt = Fnv1a32::digest(&rebuilt);
    let fingerprint_match = fingerprint_raw == fingerprint_rebuilt && rebuilt.len() == archive.len();
    if !fingerprint_match {
        return Err(CpioIterError::new(
            V604iCode::FingerprintMiss,
            n_entries,
            0,
        ));
    }

    // 双跑确定性对拍：第二走（独立迭代器）序指纹必须恒等。
    let walk2 = static_walk(archive)?;
    let deterministic = walk2.sequence_fingerprint == walk1.sequence_fingerprint
        && walk2.consumed == walk1.consumed;
    if !deterministic {
        return Err(CpioIterError::new(
            V604iCode::LedgerMiss,
            n_entries,
            0,
        ));
    }

    // 8 要素跨档累计（逐条核的全档留痕）。
    let mut element_pass_counts = [0u32; FIELD_AUDIT_ELEMENTS];
    let mut audit_passed: u32 = 0;
    let mut i: usize = 0;
    while i < walk1.entries.len() {
        let audit = &walk1.entries[i].audit;
        if audit.all_passed() {
            audit_passed = audit_passed.saturating_add(1);
        }
        let mut e: usize = 0;
        while e < FIELD_AUDIT_ELEMENTS {
            if audit.element_pass[e] {
                element_pass_counts[e] = element_pass_counts[e].saturating_add(1);
            }
            e += 1;
        }
        i += 1;
    }

    // 事件账封口：四账平事件入账。（shape 先取，避免部分移动后借整件。）
    let shape = ArchiveShape::from_walk(&walk1);
    let mut ledger = walk1.ledger;
    ledger.push_note(
        IterEventKind::SpanConserved,
        u32::MAX,
        0,
        "Σspan=消耗 且 消耗+残余=档长，守恒平",
    );
    ledger.push_note(
        IterEventKind::CountsMatched,
        u32::MAX,
        0,
        "n 条目 = n 推算数，账齐",
    );
    ledger.push_note(
        IterEventKind::FingerprintOk,
        u32::MAX,
        0,
        "双 FNV 指纹对拍平（往返一致）",
    );
    ledger.push_note(
        IterEventKind::DeterminismOk,
        u32::MAX,
        0,
        "双跑序指纹恒等（确定性平）",
    );
    let health = ledger.health();

    Ok(TraverseReport {
        empty_archive: false,
        n_entries,
        n_predicted: light.entries,
        counts_match,
        consumed_bytes: walk1.consumed,
        residual_bytes: walk1.residual,
        span_conserved,
        trailer_seen: walk1.trailer_seen,
        trailer_entry_index: walk1.trailer_entry_index,
        audit_passed_entries: audit_passed,
        audit_element_pass_counts: element_pass_counts,
        fingerprint_raw,
        fingerprint_rebuilt,
        fingerprint_match,
        deterministic,
        sequence_fingerprint: walk1.sequence_fingerprint,
        shape,
        ledger,
        health,
    })
}

// ==========================================================================
// 注入面（烂档五类检出演练）
// ==========================================================================

/// 注入案种类（判据任务书三类 + 名区病理两类，共五案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectionKind {
    /// 魔数坏注入（首字节翻转 → MagicMismatch 0x14B1）。
    BadMagic,
    /// 对齐错注入（首个垫字节置 0xFF → PadNonZero 0x14B9）。
    PadCorrupt,
    /// 越界注入（c_filesize 注水 0x100000 → DataOverrun 0x14B5）。
    FilesizeOverflow,
    /// 名 NUL 坏注入（名区末字节 NUL → 'X' → NameNulMissing 0x14B4）。
    NameNulMissing,
    /// namesize 零注入（c_namesize 清零 → NamesizeZero 0x14B3）。
    NamesizeZero,
}

impl InjectionKind {
    /// 案数。
    pub const COUNT: usize = 5;

    /// 全闭集。
    pub const ALL: [InjectionKind; InjectionKind::COUNT] = [
        InjectionKind::BadMagic,
        InjectionKind::PadCorrupt,
        InjectionKind::FilesizeOverflow,
        InjectionKind::NameNulMissing,
        InjectionKind::NamesizeZero,
    ];

    /// 期望检出码。
    pub const fn expected_code(self) -> u16 {
        match self {
            InjectionKind::BadMagic => V604iCode::MagicMismatch.code(),
            InjectionKind::PadCorrupt => V604iCode::PadNonZero.code(),
            InjectionKind::FilesizeOverflow => V604iCode::DataOverrun.code(),
            InjectionKind::NameNulMissing => V604iCode::NameNulMissing.code(),
            InjectionKind::NamesizeZero => V604iCode::NamesizeZero.code(),
        }
    }

    /// 案名（中文）。
    pub const fn name(self) -> &'static str {
        match self {
            InjectionKind::BadMagic => "魔数坏注入",
            InjectionKind::PadCorrupt => "对齐错注入",
            InjectionKind::FilesizeOverflow => "越界注入",
            InjectionKind::NameNulMissing => "名 NUL 坏注入",
            InjectionKind::NamesizeZero => "namesize 零注入",
        }
    }
}

/// 单案演练结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrillCase {
    /// 案种。
    pub kind: InjectionKind,
    /// 期望检出码。
    pub expected_code: u16,
    /// 实际检出码（未检出为 0）。
    pub actual_code: u16,
    /// 是否检出（实际码 = 期望码）。
    pub detected: bool,
}

/// 五案演练报告。
#[derive(Clone, Debug)]
pub struct DrillReport {
    /// 五案结果（序 = InjectionKind::ALL 序）。
    pub cases: [DrillCase; InjectionKind::COUNT],
    /// 全检出判定（好档误伤 0：基线档自身先过一遍全走）。
    pub all_detected: bool,
}

/// 找归档内首个对齐垫字节位置（轻走全档扫垫；无垫返回 None）。
fn first_pad_position(archive: &[u8]) -> Option<usize> {
    let mut pos: usize = 0;
    let total = archive.len();
    let mut guard: u32 = 0;
    while pos + HEADER_LEN <= total && guard < 1_000_000 {
        guard = guard.saturating_add(1);
        let namesize = match parse_hex8(
            &archive[pos + FIELD_OFFSETS[FIELD_NAMESIZE]..pos + FIELD_OFFSETS[FIELD_NAMESIZE] + FIELD_HEX_LEN],
        ) {
            Ok(v) => v,
            Err(_) => return None,
        };
        let filesize = match parse_hex8(
            &archive[pos + FIELD_OFFSETS[FIELD_FILESIZE]..pos + FIELD_OFFSETS[FIELD_FILESIZE] + FIELD_HEX_LEN],
        ) {
            Ok(v) => v,
            Err(_) => return None,
        };
        if namesize == 0 {
            return None;
        }
        let name_start = pos + HEADER_LEN;
        let data_start = align_up(name_start + namesize as usize);
        let name_pad = pad_needed((pos + HEADER_LEN) as u32 + namesize);
        if name_pad > 0 && name_end_in_bounds(name_start, namesize, total) {
            return Some(name_start + namesize as usize);
        }
        let data_pad = pad_needed(filesize);
        let data_end = data_start.checked_add(filesize as usize)?;
        if data_pad > 0 && data_end <= total {
            return Some(data_end);
        }
        if data_end.checked_add(data_pad as usize)? > total {
            return None;
        }
        pos = data_end + data_pad as usize;
    }
    None
}

fn name_end_in_bounds(name_start: usize, namesize: u32, total: usize) -> bool {
    match name_start.checked_add(namesize as usize) {
        Some(e) => e <= total,
        None => false,
    }
}

/// 五案注入检出演练：基线档先过全走（好档误伤 0），逐案腐化克隆档并确认
/// 期望码被检出。基线档必须为可全走的 canonical newc 归档。
pub fn run_injection_drill(archive: &[u8]) -> Result<DrillReport, CpioIterError> {
    // 基线好档误伤 0：先跑一遍全走，红即演练前置失守。
    traverse_archive(archive)?;

    let mut cases = [DrillCase {
        kind: InjectionKind::BadMagic,
        expected_code: 0,
        actual_code: 0,
        detected: false,
    }; InjectionKind::COUNT];

    let mut ci: usize = 0;
    while ci < InjectionKind::COUNT {
        let kind = InjectionKind::ALL[ci];
        let mut corrupted = Vec::from(archive);
        let expected = kind.expected_code();
        let actual: u16 = match kind {
            InjectionKind::BadMagic => {
                if corrupted.is_empty() {
                    0
                } else {
                    corrupted[0] = if corrupted[0] == b'0' { b'1' } else { b'0' };
                    match traverse_archive(&corrupted) {
                        Err(err) => err.code_value(),
                        Ok(_) => 0,
                    }
                }
            }
            InjectionKind::NamesizeZero => {
                let off = FIELD_OFFSETS[FIELD_NAMESIZE];
                if write_hex8_into(&mut corrupted, off, 0) {
                    match traverse_archive(&corrupted) {
                        Err(err) => err.code_value(),
                        Ok(_) => 0,
                    }
                } else {
                    0
                }
            }
            InjectionKind::FilesizeOverflow => {
                let header_opt = match CpioNewcHeader::parse_at(&corrupted, 0) {
                    Ok(h) => Some(h),
                    Err(_) => None,
                };
                match header_opt {
                    None => 0, // 解析失败即演练面失守（基线已过不该发生）。
                    Some(header) => {
                        if header.c_filesize == 0 && corrupted.len() <= HEADER_LEN {
                            0
                        } else {
                            let inflated = header.c_filesize.saturating_add(0x0010_0000);
                            let off = FIELD_OFFSETS[FIELD_FILESIZE];
                            if write_hex8_into(&mut corrupted, off, inflated) {
                                match traverse_archive(&corrupted) {
                                    Err(err) => err.code_value(),
                                    Ok(_) => 0,
                                }
                            } else {
                                0
                            }
                        }
                    }
                }
            }
            InjectionKind::NameNulMissing => {
                let header_opt = match CpioNewcHeader::parse_at(&corrupted, 0) {
                    Ok(h) => Some(h),
                    Err(_) => None,
                };
                match header_opt {
                    None => 0,
                    Some(header) => {
                        let name_start = HEADER_LEN;
                        let nul_at = match name_start.checked_add(header.c_namesize as usize) {
                            Some(v) => v.saturating_sub(1),
                            None => 0usize,
                        };
                        if header.c_namesize >= 1
                            && nul_at < corrupted.len()
                            && corrupted[nul_at] == NAME_TERMINATOR
                        {
                            corrupted[nul_at] = b'X';
                            match traverse_archive(&corrupted) {
                                Err(err) => err.code_value(),
                                Ok(_) => 0,
                            }
                        } else {
                            0
                        }
                    }
                }
            }
            InjectionKind::PadCorrupt => match first_pad_position(&corrupted) {
                Some(pad_at) if pad_at < corrupted.len() => {
                    corrupted[pad_at] = 0xFF;
                    match traverse_archive(&corrupted) {
                        Err(err) => err.code_value(),
                        Ok(_) => 0,
                    }
                }
                _ => 0,
            },
        };
        cases[ci] = DrillCase {
            kind,
            expected_code: expected,
            actual_code: actual,
            detected: actual == expected,
        };
        ci += 1;
    }

    let all_detected = {
        let mut ok = true;
        let mut i: usize = 0;
        while i < cases.len() {
            if !cases[i].detected {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    Ok(DrillReport {
        cases,
        all_detected,
    })
}

// ==========================================================================
// 冒烟与注入测试（隔离探针 cfg(test)；内核构建不含本段）
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------- 测试侧独立产档器（与重编码路径异构，保证往返真实） --------

    /// 测试侧 u32 → 8 位小写 hex 字符串（走 core::fmt，不走 encode_hex8）。
    fn hex8s(value: u32) -> String {
        format!("{:08x}", value)
    }

    /// 测试侧 canonical newc 条目产档（独立实现：format! 拼字段 + 逐垫对齐）。
    fn build_entry(name: &str, mode: u32, nlink: u32, ino: u32, data: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"070701");
        out.extend_from_slice(hex8s(ino).as_bytes()); // c_ino
        out.extend_from_slice(hex8s(mode).as_bytes()); // c_mode
        out.extend_from_slice(hex8s(0).as_bytes()); // c_uid
        out.extend_from_slice(hex8s(0).as_bytes()); // c_gid
        out.extend_from_slice(hex8s(nlink).as_bytes()); // c_nlink
        out.extend_from_slice(hex8s(1_700_000_000).as_bytes()); // c_mtime
        out.extend_from_slice(hex8s(data.len() as u32).as_bytes()); // c_filesize
        out.extend_from_slice(hex8s(0).as_bytes()); // c_devmajor
        out.extend_from_slice(hex8s(0).as_bytes()); // c_devminor
        out.extend_from_slice(hex8s(0).as_bytes()); // c_rdevmajor
        out.extend_from_slice(hex8s(0).as_bytes()); // c_rdevminor
        let namesize = name.len() as u32 + 1;
        out.extend_from_slice(hex8s(namesize).as_bytes()); // c_namesize
        out.extend_from_slice(hex8s(0).as_bytes()); // c_check
        assert_eq!(out.len(), HEADER_LEN, "测试产档头必须恰 110 字节");
        out.extend_from_slice(name.as_bytes());
        out.push(NAME_TERMINATOR);
        while out.len() % ARCHIVE_ALIGNMENT as usize != 0 {
            out.push(0); // 名区垫（把数据区起点收口到 4 界）
        }
        out.extend_from_slice(data);
        while out.len() % ARCHIVE_ALIGNMENT as usize != 0 {
            out.push(0); // 数据垫
        }
        out
    }

    fn trailer_entry() -> Vec<u8> {
        build_entry(TRAILER_NAME, 0, 1, 0, &[])
    }

    /// 标准测试档：目录 + 常规文件 + 符号链接 + TRAILER（canonical 全过基线）。
    fn good_archive() -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&build_entry("init.d", 0o040755, 2, 1, &[]));
        out.extend_from_slice(&build_entry("init", 0o100755, 1, 2, b"\x7fELF-fake-kernel-bytes"));
        out.extend_from_slice(&build_entry("init.link", 0o120777, 1, 3, b"init"));
        out.extend_from_slice(&trailer_entry());
        out
    }

    fn expect_code(result: Result<TraverseReport, CpioIterError>, want: u16) {
        match result {
            Err(err) => assert_eq!(
                err.code_value(),
                want,
                "期望码 {:04X}，实得 {:04X}（{}）",
                want,
                err.code_value(),
                err.describe()
            ),
            Ok(_) => panic_nonetest(want),
        }
    }

    /// 测试断言辅助（仅测试路径；替代 unwrap 的显性失败）。
    fn panic_nonetest(want: u16) {
        assert!(false, "期望检出码 {:04X}，但遍历意外全过", want);
    }

    // ---------------- 诊断闭集账 ----------------

    #[test]
    fn diag_codes_closed_set() {
        assert_eq!(V604iCode::ALL.len(), 16);
        let mut i: usize = 0;
        while i < V604iCode::ALL.len() {
            let code = V604iCode::ALL[i];
            assert_eq!(code.code(), V604I_SEG_BASE + i as u16, "段内码须连续");
            assert_eq!(V604iCode::from_code(code.code()), Some(code), "码反查闭环");
            assert!(!code.name().is_empty());
            assert!(code.detail().chars().count() >= 16, "诊断须中文可操作");
            i += 1;
        }
        assert_eq!(V604iCode::from_code(0x14A0), None, "段外码必须落空");
        assert_eq!(V604iCode::from_code(0x14C0), None, "段外码必须落空");
    }

    #[test]
    fn error_accounting_fields() {
        let err = CpioIterError::new(V604iCode::PadNonZero, 3, 0x1234);
        assert_eq!(err.code(), V604iCode::PadNonZero);
        assert_eq!(err.entry_index(), 3);
        assert_eq!(err.offset(), 0x1234);
        assert_eq!(err.detail(), V604iCode::PadNonZero.detail());
        let described = err.describe();
        assert!(described.contains("14B9"), "describe 须带段内码");
        assert!(described.contains("条目3"), "describe 须带条目定位");
    }

    // ---------------- 手写 hex 编解码 ----------------

    #[test]
    fn hex_roundtrip() {
        assert_eq!(parse_hex8(b"00000000"), Ok(0));
        assert_eq!(parse_hex8(b"ffffffff"), Ok(0xFFFFFFFF));
        assert_eq!(parse_hex8(b"07070100"), Ok(0x0707_0100));
        assert_eq!(parse_hex8(b"ABCDEF01"), Ok(0xABCD_EF01));
        assert_eq!(parse_hex8(b"1234567"), Err(V604iCode::HexNonHex));
        assert_eq!(parse_hex8(b"1234567g"), Err(V604iCode::HexNonHex));
        assert_eq!(parse_hex8(b"1234 678"), Err(V604iCode::HexNonHex));
        assert_eq!(encode_hex8(0), *b"00000000");
        assert_eq!(encode_hex8(0xDEAD_BEEF), *b"deadbeef");
        let mut buf = *b"XXXXXXXXXXXXXXXX";
        assert!(write_hex8_into(&mut buf, 8, 0x0000_00ff));
        assert_eq!(&buf[8..16], b"000000ff");
        assert!(!write_hex8_into(&mut buf, 15, 0), "越界写须拒");
        assert_eq!(hex_digit_value(b'0'), Some(0));
        assert_eq!(hex_digit_value(b'9'), Some(9));
        assert_eq!(hex_digit_value(b'a'), Some(10));
        assert_eq!(hex_digit_value(b'f'), Some(15));
        assert_eq!(hex_digit_value(b'g'), None);
        assert_eq!(hex_digit_value(b'/'), None);
    }

    #[test]
    fn fnv_known_vectors() {
        // FNV-1a 32 官方公布向量（Landon Curt Noll 测试向量表）。
        assert_eq!(Fnv1a32::digest(b""), 0x811C_9DC5);
        assert_eq!(Fnv1a32::digest(b"a"), 0xE40C_292C);
        assert_eq!(Fnv1a32::digest(b"foobar"), 0xBF9C_F968);
    }

    // ---------------- 头解析 ----------------

    #[test]
    fn header_parse_and_fields() {
        let archive = good_archive();
        let header = CpioNewcHeader::parse_at(&archive, 0);
        let header = match header {
            Ok(h) => h,
            Err(err) => {
                assert!(false, "基线头解析失败：{}", err.describe());
                return;
            }
        };
        // 基线目录条目字段逐一对账（与测试产档器写定值核对）。
        assert_eq!(header.c_ino, 1);
        assert_eq!(header.c_mode, 0o040755);
        assert_eq!(header.c_uid, 0);
        assert_eq!(header.c_gid, 0);
        assert_eq!(header.c_nlink, 2);
        assert_eq!(header.c_mtime, 1_700_000_000);
        assert_eq!(header.c_filesize, 0);
        assert_eq!(header.c_namesize, 7); // "init.d" 6 字 + NUL
        assert_eq!(header.c_check, 0);
        // 13 字段打包序 = 规格序。
        let fields = header.fields();
        assert_eq!(fields.len(), 13);
        assert_eq!(fields[0], header.c_ino);
        assert_eq!(fields[6], header.c_filesize);
        assert_eq!(fields[11], header.c_namesize);
        assert_eq!(fields[12], header.c_check);
        // 类型闭集判别 + 权限位解码。
        assert_eq!(header.file_type(), FileType::Directory);
        let perms = header.permissions();
        assert_eq!(perms.render(), "rwxr-xr-x");
        assert!(!perms.setuid());
        assert!(!perms.sticky());
        // 名文本长度派生。
        assert_eq!(header.name_text_len(), 6);
        // 字段偏移表账：末字段止于 110。
        assert_eq!(FIELD_OFFSETS[12] + 8, HEADER_LEN);
        assert_eq!(FIELD_NAMES.len(), 13);
        assert_eq!(FIELD_NAMES[11], "c_namesize");
    }

    #[test]
    fn header_short_reject() {
        // 60 字节：不足 110 → HeadShort。
        let mut short: Vec<u8> = Vec::new();
        short.extend_from_slice(&NEWC_MAGIC);
        while short.len() < 60 {
            short.push(b'0');
        }
        match CpioNewcHeader::parse_at(&short, 0) {
            Err(err) => assert_eq!(err.code(), V604iCode::HeadShort),
            Ok(_) => assert!(false, "短头必须拒收"),
        }
        match traverse_archive(&short) {
            Err(err) => assert_eq!(err.code(), V604iCode::HeadShort),
            Ok(_) => assert!(false, "短头必须拒收"),
        }
    }

    #[test]
    fn header_magic_reject() {
        let mut archive = good_archive();
        archive[3] = b'8'; // "070701" → "070781"
        match traverse_archive(&archive) {
            Err(err) => {
                assert_eq!(err.code(), V604iCode::MagicMismatch);
                assert_eq!(err.offset(), 3);
            }
            Ok(_) => assert!(false, "魔数坏必须拒收"),
        }
    }

    #[test]
    fn header_hex_reject() {
        let mut archive = good_archive();
        let off = FIELD_OFFSETS[0]; // c_ino 字段
        assert_eq!(off, 6);
        archive[off + 5] = b'z'; // hex 域内坏字符
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::HexNonHex),
            Ok(_) => assert!(false, "hex 坏必须拒收"),
        }
    }

    // ---------------- 文件类型闭集 ----------------

    #[test]
    fn file_type_closed_set() {
        assert_eq!(FileType::from_mode(0o010644), FileType::Fifo);
        assert_eq!(FileType::from_mode(0o020600), FileType::CharDev);
        assert_eq!(FileType::from_mode(0o040755), FileType::Directory);
        assert_eq!(FileType::from_mode(0o060600), FileType::BlockDev);
        assert_eq!(FileType::from_mode(0o100644), FileType::Regular);
        assert_eq!(FileType::from_mode(0o120777), FileType::Symlink);
        assert_eq!(FileType::from_mode(0o140777), FileType::Socket);
        assert_eq!(FileType::from_mode(0o170644), FileType::Unknown);
        assert_eq!(FileType::from_mode(0o000644), FileType::Unknown);
        assert_eq!(FileType::from_mode(0xFFFFFFFF), FileType::Unknown);
        assert_eq!(FileType::KNOWN_COUNT, 7);
        // Unknown 直方图独立观察位；七类入图。
        assert_eq!(FileType::Fifo.id(), 0);
        assert_eq!(FileType::Socket.id(), 6);
        assert_eq!(FileType::Unknown.id(), 7);
    }

    #[test]
    fn permission_render_special_bits() {
        assert_eq!(PermissionBits::from_mode(0o100644).render(), "rw-r--r--");
        assert_eq!(PermissionBits::from_mode(0o100755).render(), "rwxr-xr-x");
        // setuid（0o4755）。
        let su = PermissionBits::from_mode(0o4755);
        assert!(su.setuid());
        assert_eq!(su.render(), "rwsr-xr-x");
        // setgid（0o2755）。
        let sg = PermissionBits::from_mode(0o2755);
        assert!(sg.setgid());
        assert_eq!(sg.render(), "rwxr-sr-x");
        // sticky（0o1777）。
        let st = PermissionBits::from_mode(0o1777);
        assert!(st.sticky());
        assert_eq!(st.render(), "rwxrwxrwt");
    }

    // ---------------- 对齐推进 ----------------

    #[test]
    fn alignment_advance_real() {
        assert_eq!(pad_needed(0), 0);
        assert_eq!(pad_needed(1), 3);
        assert_eq!(pad_needed(2), 2);
        assert_eq!(pad_needed(3), 1);
        assert_eq!(pad_needed(4), 0);
        assert_eq!(pad_needed(110), 2);
        assert_eq!(align_up(110), 112);
        assert_eq!(align_up(114), 116);
        assert_eq!(align_up(0), 0);
        assert_eq!(align_up(4), 4);
    }

    // ---------------- 冒烟：真实归档遍历总账 ----------------

    #[test]
    fn smoke_traverse_full_ledger() {
        let archive = good_archive();
        let report = match traverse_archive(&archive) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "基线档必须全过：{}", err.describe());
                return;
            }
        };
        // 遍历账齐：n 条目 = n 推算数。
        assert_eq!(report.n_entries, 3);
        assert_eq!(report.n_predicted, 3);
        assert!(report.counts_match);
        // 头字段账：3 条目 8 要素逐条全过。
        assert_eq!(report.audit_passed_entries, 3);
        let mut e: usize = 0;
        while e < FIELD_AUDIT_ELEMENTS {
            assert_eq!(
                report.audit_element_pass_counts[e], 3,
                "要素 {}（{}）须 3/3 全过",
                e, AUDIT_ELEMENT_NAMES[e]
            );
            e += 1;
        }
        // 字节守恒：消耗 + 残余 = 档长，残余 = 0。
        assert!(report.span_conserved);
        assert_eq!(report.consumed_bytes, archive.len() as u64);
        assert_eq!(report.residual_bytes, 0);
        // TRAILER 识别：为第 3 个受理条目之后的头（序号 = 3）。
        assert!(report.trailer_seen);
        assert_eq!(report.trailer_entry_index, Some(3));
        // 抽样对拍：双 FNV 指纹平 + 往返一致。
        assert!(report.fingerprint_match);
        assert_eq!(report.fingerprint_raw, report.fingerprint_rebuilt);
        assert_eq!(report.fingerprint_raw, Fnv1a32::digest(&archive));
        // 双跑确定性。
        assert!(report.deterministic);
        // 健康面：零异常。
        assert!(report.health.ok);
        assert_eq!(report.health.anomaly_count, 0);
        assert_eq!(report.health.events, report.ledger.len() as u32);
        // 形态概括。
        let shape = &report.shape;
        assert_eq!(shape.n_entries, 3);
        assert_eq!(shape.type_histogram[FileType::Directory.id()], 1);
        assert_eq!(shape.type_histogram[FileType::Regular.id()], 1);
        assert_eq!(shape.type_histogram[FileType::Symlink.id()], 1);
        assert_eq!(shape.unknown_types, 0);
        assert_eq!(shape.max_name_len, 9); // "init.link"
        assert_eq!(shape.min_name_len, 4); // "init"
        assert_eq!(shape.total_data_bytes, 26); // 0 + 22 + 4
        // 事件账：正常事件链 + 指纹可复算（双跑账指纹恒等）。
        assert!(report.ledger.len() >= 3 * 3 + 1 + 4);
        assert_eq!(report.ledger.anomaly_count(), 0);
    }

    #[test]
    fn smoke_iterator_incremental() {
        let archive = good_archive();
        let mut iter = CpioIterator::from_slice(&archive);
        assert!(iter.is_empty_archive() == false);
        // 条目 0：目录。
        let e0 = match iter.next_entry() {
            Ok(Some(e)) => e,
            other => {
                assert!(false, "条目 0 须受理：{:?}", other.is_err());
                return;
            }
        };
        assert_eq!(e0.index, 0);
        assert_eq!(e0.name, "init.d");
        assert_eq!(e0.file_type(), FileType::Directory);
        assert!(e0.audit.all_passed());
        assert_eq!(e0.header_offset, 0);
        assert_eq!(e0.name_offset, HEADER_LEN as u32);
        assert_eq!(e0.data_offset, 120); // align4(110+7)=120，名垫 3 字节
        // 条目 1：常规文件，带数据窗口。
        let e1 = match iter.next_entry() {
            Ok(Some(e)) => e,
            other => {
                assert!(false, "条目 1 须受理：{:?}", other.is_err());
                return;
            }
        };
        assert_eq!(e1.index, 1);
        assert_eq!(e1.name, "init");
        assert_eq!(e1.data_len, 22);
        let window = e1.data_window(&archive);
        match window {
            Some(d) => assert_eq!(d, b"\x7fELF-fake-kernel-bytes"),
            None => assert!(false, "数据窗口必须开窗"),
        }
        // 条目 2：符号链接。
        let e2 = match iter.next_entry() {
            Ok(Some(e)) => e,
            other => {
                assert!(false, "条目 2 须受理：{:?}", other.is_err());
                return;
            }
        };
        assert_eq!(e2.name, "init.link");
        assert_eq!(e2.file_type(), FileType::Symlink);
        // 第 4 问：TRAILER 收档 → None（非错误）。
        match iter.next_entry() {
            Ok(None) => {}
            other => {
                assert!(false, "TRAILER 后应收档：{:?}", other.is_err());
                return;
            }
        }
        assert!(iter.trailer_seen());
        assert_eq!(iter.next_index(), 3);
        assert_eq!(iter.consumed_bytes(), archive.len() as u64);
        assert_eq!(iter.ino_duplicates(), 0);
        // 收档后幂等再入（安全面）。
        match iter.next_entry() {
            Ok(None) => {}
            _ => assert!(false, "收档后再入必须幂等 None"),
        }
        assert!(iter.health().ok);
    }

    #[test]
    fn smoke_span_arithmetic_per_entry() {
        // 逐条目 span = 110 + namesize + 名垫 + filesize + 数据垫。
        let archive = good_archive();
        let mut iter = CpioIterator::from_slice(&archive);
        let mut prev_end: u32 = 0;
        let mut seen: u32 = 0;
        loop {
            match iter.next_entry() {
                Ok(Some(entry)) => {
                    assert_eq!(entry.header_offset, prev_end, "条目头偏移须连续");
                    prev_end = prev_end.saturating_add(entry.total_span_u64() as u32);
                    // 对齐垫派生账。
                    let np = pad_needed(entry.header_offset + HEADER_LEN as u32 + entry.header.c_namesize);
                    assert_eq!(entry.name_pad as u32, np);
                    assert_eq!(entry.data_pad as u32, pad_needed(entry.header.c_filesize));
                    assert_eq!(entry.total_span_u64() % 4, 0, "条目 span 须 4 对齐");
                    seen = seen.saturating_add(1);
                }
                Ok(None) => break,
                Err(err) => {
                    assert!(false, "遍历失败：{}", err.describe());
                    return;
                }
            }
        }
        assert_eq!(seen, 3);
        // 收档 span 复算：trailer span = 110 + pad4(110+11) = 124（名 "TRAILER!!!"）。
        let trailer_span_expected = HEADER_LEN as u64
            + (TRAILER_NAME.len() as u32 + 1) as u64
            + pad_needed(HEADER_LEN as u32 + TRAILER_NAME.len() as u32 + 1) as u64;
        assert_eq!(iter.consumed_bytes(), prev_end as u64 + trailer_span_expected);
        assert_eq!(iter.consumed_bytes(), archive.len() as u64);
    }

    // ---------------- 空归档 / 仅 TRAILER ----------------

    #[test]
    fn empty_archive_explicit_state() {
        let report = match traverse_archive(&[]) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "空归档须显性空态非错误：{}", err.describe());
                return;
            }
        };
        assert!(report.empty_archive);
        assert_eq!(report.n_entries, 0);
        assert_eq!(report.n_predicted, 0);
        assert!(report.counts_match);
        assert!(report.span_conserved);
        assert!(!report.trailer_seen);
        assert!(report.health.ok);
        // 事件账含 EmptyArchive 空态事件。
        let mut found = false;
        let mut i: usize = 0;
        let events = report.ledger.events();
        while i < events.len() {
            if events[i].kind == IterEventKind::EmptyArchive {
                found = true;
            }
            i += 1;
        }
        assert!(found, "空态事件必须入账");
    }

    #[test]
    fn trailer_only_archive() {
        let archive = trailer_entry();
        let report = match traverse_archive(&archive) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "仅 TRAILER 档须全过：{}", err.describe());
                return;
            }
        };
        assert_eq!(report.n_entries, 0);
        assert_eq!(report.n_predicted, 0);
        assert!(report.trailer_seen);
        assert_eq!(report.trailer_entry_index, Some(0));
        assert_eq!(report.residual_bytes, 0);
        assert!(report.health.ok);
    }

    // ---------------- TRAILER 与残余账注 ----------------

    #[test]
    fn trailer_residue_noted() {
        // TRAILER 之后拖 4 字节零残余：账注非零残余（合法注记，非错误）。
        let mut archive = good_archive();
        archive.extend_from_slice(&[0u8; 4]);
        let report = match traverse_archive(&archive) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "带残余档须全过：{}", err.describe());
                return;
            }
        };
        assert_eq!(report.residual_bytes, 4);
        assert!(report.health.ok, "非零残余只账注不判红");
        let mut noted = false;
        let events = report.ledger.events();
        let mut i: usize = 0;
        while i < events.len() {
            if events[i].kind == IterEventKind::ResidualNoted {
                noted = true;
            }
            i += 1;
        }
        assert!(noted, "残余账注事件必须入账");
    }

    #[test]
    fn trailer_missing_reject() {
        // 好档去掉 TRAILER（截去尾条目）→ TrailerMissing。
        let mut archive = good_archive();
        let tlen = trailer_entry().len();
        let cut = archive.len() - tlen;
        archive.truncate(cut);
        expect_code(traverse_archive(&archive), V604iCode::TrailerMissing.code());
    }

    // ---------------- 边界防线 ----------------

    #[test]
    fn data_overrun_reject() {
        // c_filesize 注水越界。
        let mut archive = good_archive();
        let inflated = 0x0010_0000u32;
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[FIELD_FILESIZE], inflated));
        expect_code(
            traverse_archive(&archive),
            V604iCode::DataOverrun.code(),
        );
    }

    #[test]
    fn name_overrun_reject() {
        // c_namesize 注水致名区越界。
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[FIELD_NAMESIZE], 0x0020_0000));
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::DataOverrun),
            Ok(_) => assert!(false, "名区越界必须拒收"),
        }
    }

    #[test]
    fn cursor_past_end_reject() {
        // 有条目推进后档被截断（余 40 字节不成头，且无 TRAILER）→ CursorPastEnd。
        let mut archive = good_archive();
        let tlen = trailer_entry().len();
        let cut = archive.len() - tlen;
        archive.truncate(cut);
        archive.extend_from_slice(&[b'0'; 40]);
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::CursorPastEnd),
            Ok(_) => assert!(false, "残头必须拒收"),
        }
        // 轻走推算同判。
        let light = count_entries_light(&archive);
        assert_eq!(light.stop_code, Some(V604iCode::CursorPastEnd.code()));
    }

    #[test]
    fn namesize_zero_reject() {
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[FIELD_NAMESIZE], 0));
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::NamesizeZero),
            Ok(_) => assert!(false, "namesize=0 必须拒收"),
        }
    }

    #[test]
    fn name_nul_missing_reject() {
        let mut archive = good_archive();
        let nul_at = HEADER_LEN + "init.d".len(); // 名区末字节（NUL 位）
        assert_eq!(archive[nul_at], NAME_TERMINATOR);
        archive[nul_at] = b'!';
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::NameNulMissing),
            Ok(_) => assert!(false, "名无 NUL 必须拒收"),
        }
    }

    #[test]
    fn name_control_byte_reject() {
        // 名文本含控制字节（0x01）→ namesize 要素红（FieldAuditMiss）。
        let mut archive = good_archive();
        archive[HEADER_LEN] = 0x01;
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::FieldAuditMiss),
            Ok(_) => assert!(false, "名控制字节必须拒收"),
        }
    }

    #[test]
    fn pad_nonzero_reject() {
        // 首条目名垫字节置 0xFF：110+7=117 → 垫 3 字节于 [117,120)。
        let mut archive = good_archive();
        assert_eq!(pad_needed(HEADER_LEN as u32 + 7), 3);
        archive[117] = 0xFF;
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::PadNonZero),
            Ok(_) => assert!(false, "垫字节非零必须拒收"),
        }
    }

    // ---------------- 8 要素账逐条红 ----------------

    #[test]
    fn audit_mode_unknown_type_reject() {
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[1], 0o170644));
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::ModeTypeUnknown),
            Ok(_) => assert!(false, "类型落闭集外必须拒收"),
        }
    }

    #[test]
    fn audit_mode_perm_domain_reject() {
        // 权限域越 0o7777（高噪声位混入低 12 位域之外）。
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[1], 0o040755 | 0x8000_0000));
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::ModeTypeUnknown),
            Ok(_) => assert!(false, "mode 高位噪声必须拒收"),
        }
    }

    #[test]
    fn audit_nlink_domain_reject() {
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[4], 0)); // nlink = 0
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::NlinkIllegal),
            Ok(_) => assert!(false, "nlink=0 必须拒收"),
        }
        // 越上界 0x10000。
        let mut archive2 = good_archive();
        assert!(write_hex8_into(&mut archive2, FIELD_OFFSETS[4], 0x0001_0000));
        match traverse_archive(&archive2) {
            Err(err) => assert_eq!(err.code(), V604iCode::NlinkIllegal),
            Ok(_) => assert!(false, "nlink 越域必须拒收"),
        }
    }

    #[test]
    fn audit_uidgid_domain_reject() {
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[2], 0x0001_0000)); // uid 越域
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::FieldAuditMiss),
            Ok(_) => assert!(false, "uid 越域必须拒收"),
        }
    }

    #[test]
    fn audit_mtime_sentinel_reject() {
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[5], 0xFFFF_FFFF));
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::FieldAuditMiss),
            Ok(_) => assert!(false, "mtime 哨兵必须拒收"),
        }
    }

    #[test]
    fn audit_check_nonzero_reject() {
        let mut archive = good_archive();
        assert!(write_hex8_into(&mut archive, FIELD_OFFSETS[FIELD_CHECK], 1));
        match traverse_archive(&archive) {
            Err(err) => assert_eq!(err.code(), V604iCode::CheckFieldNonZero),
            Ok(_) => assert!(false, "c_check 非 0 必须拒收"),
        }
    }

    // ---------------- 往返一致（重编码 vs 原档） ----------------

    #[test]
    fn rebuild_roundtrip_byte_equal() {
        let archive = good_archive();
        let walk = match static_walk(&archive) {
            Ok(w) => w,
            Err(err) => {
                assert!(false, "全走失败：{}", err.describe());
                return;
            }
        };
        let rebuilt = match rebuild_archive(&walk.records) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "重编码失败：{}", err.describe());
                return;
            }
        };
        // 往返一致自证：逐字节等长等值（非仅指纹）。
        assert_eq!(rebuilt.len(), archive.len());
        let mut i: usize = 0;
        while i < archive.len() {
            assert_eq!(rebuilt[i], archive[i], "首个差异字节 @{}", i);
            i += 1;
        }
        assert_eq!(Fnv1a32::digest(&rebuilt), Fnv1a32::digest(&archive));
    }

    #[test]
    fn rebuild_detects_record_inconsistency() {
        // 档案面账红注入：namesize 与名长不一致 → FingerprintMiss 拒编码。
        let archive = good_archive();
        let walk = match static_walk(&archive) {
            Ok(w) => w,
            Err(_) => {
                assert!(false, "基线全走失败");
                return;
            }
        };
        let mut bad_records = walk.records.clone();
        bad_records[0].header.c_namesize = 99;
        match rebuild_archive(&bad_records) {
            Err(err) => assert_eq!(err.code(), V604iCode::FingerprintMiss),
            Ok(_) => assert!(false, "账面不一致必须拒编码"),
        }
        let mut bad_records2 = walk.records.clone();
        bad_records2[1].data.push(0xFF); // 数据与 filesize 失账
        match rebuild_archive(&bad_records2) {
            Err(err) => assert_eq!(err.code(), V604iCode::FingerprintMiss),
            Ok(_) => assert!(false, "数据失账必须拒编码"),
        }
    }

    // ---------------- 轻走推算（独立第二账） ----------------

    #[test]
    fn light_count_matches_audit_walk() {
        let archive = good_archive();
        let light = count_entries_light(&archive);
        assert_eq!(light.entries, 3);
        assert!(light.reached_trailer);
        assert_eq!(light.consumed, archive.len() as u64);
        assert_eq!(light.stop_code, None);
        // 空档轻走。
        let empty = count_entries_light(&[]);
        assert_eq!(empty.entries, 0);
        assert!(!empty.reached_trailer);
    }

    #[test]
    fn light_count_stops_on_corruption() {
        let mut archive = good_archive();
        archive[2] = b'9'; // 魔数坏
        let light = count_entries_light(&archive);
        assert_eq!(light.stop_code, Some(V604iCode::MagicMismatch.code()));
        assert_eq!(light.entries, 0);
    }

    // ---------------- 双跑确定性 ----------------

    #[test]
    fn determinism_double_run() {
        let archive = good_archive();
        let w1 = match static_walk(&archive) {
            Ok(w) => w,
            Err(err) => {
                assert!(false, "跑 1 失败：{}", err.describe());
                return;
            }
        };
        let w2 = match static_walk(&archive) {
            Ok(w) => w,
            Err(err) => {
                assert!(false, "跑 2 失败：{}", err.describe());
                return;
            }
        };
        assert_eq!(w1.sequence_fingerprint, w2.sequence_fingerprint);
        assert_eq!(w1.consumed, w2.consumed);
        assert_eq!(w1.ledger.fingerprint(), w2.ledger.fingerprint());
        // 事件账双跑逐条恒等。
        assert_eq!(w1.ledger.len(), w2.ledger.len());
        let ev1 = w1.ledger.events();
        let ev2 = w2.ledger.events();
        let mut i: usize = 0;
        while i < ev1.len() {
            assert_eq!(ev1[i].kind, ev2[i].kind);
            assert_eq!(ev1[i].note, ev2[i].note);
            i += 1;
        }
        // 增量迭代器路线与全走路线序指纹恒等（同一推进引擎双形态）。
        let mut iter = CpioIterator::from_slice(&archive);
        let mut seq = Fnv1a32::new();
        loop {
            match iter.next_entry() {
                Ok(Some(entry)) => {
                    seq.push_u32le(entry.index);
                    seq.push_bytes(entry.name.as_bytes());
                    seq.push(0);
                    let fields = entry.header.fields();
                    let mut fi: usize = 0;
                    while fi < fields.len() {
                        seq.push_u32le(fields[fi]);
                        fi += 1;
                    }
                    if let Some(d) = entry.data_window(&archive) {
                        seq.push_bytes(d);
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    assert!(false, "增量遍历失败：{}", err.describe());
                    return;
                }
            }
        }
        assert_eq!(seq.finish(), w1.sequence_fingerprint);
    }

    // ---------------- 注入面五案演练 ----------------

    #[test]
    fn injection_drill_all_detected() {
        let archive = good_archive();
        let report = match run_injection_drill(&archive) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "演练基线必须全过：{}", err.describe());
                return;
            }
        };
        assert_eq!(report.cases.len(), InjectionKind::COUNT);
        let mut i: usize = 0;
        while i < report.cases.len() {
            let case = &report.cases[i];
            assert!(
                case.detected,
                "案 {}（{}）未检出：期望 {:04X} 实得 {:04X}",
                i,
                case.kind.name(),
                case.expected_code,
                case.actual_code
            );
            i += 1;
        }
        assert!(report.all_detected);
    }

    #[test]
    fn injection_individual_cases() {
        let base = good_archive();
        // 案 1：魔数坏。
        let mut m = base.clone();
        m[0] = b'1';
        expect_code(traverse_archive(&m), V604iCode::MagicMismatch.code());
        // 案 2：对齐错（垫 0xFF）。
        let mut p = base.clone();
        p[117] = 0xFF;
        expect_code(traverse_archive(&p), V604iCode::PadNonZero.code());
        // 案 3：越界（filesize 注水）。
        let mut o = base.clone();
        assert!(write_hex8_into(&mut o, FIELD_OFFSETS[FIELD_FILESIZE], 0x0010_0000));
        expect_code(traverse_archive(&o), V604iCode::DataOverrun.code());
        // 案 4：名 NUL 坏。
        let mut n = base.clone();
        n[HEADER_LEN + 6] = b'X';
        expect_code(traverse_archive(&n), V604iCode::NameNulMissing.code());
        // 案 5：namesize 零。
        let mut z = base.clone();
        assert!(write_hex8_into(&mut z, FIELD_OFFSETS[FIELD_NAMESIZE], 0));
        expect_code(traverse_archive(&z), V604iCode::NamesizeZero.code());
    }

    #[test]
    fn good_archive_not_falsely_flagged() {
        // 好档误伤 0：多种合法档全走全绿。
        // ① 标准档。
        assert!(traverse_archive(&good_archive()).is_ok());
        // ② 无数据条目档（纯目录树）。
        let mut tree: Vec<u8> = Vec::new();
        tree.extend_from_slice(&build_entry("a", 0o040755, 1, 1, &[]));
        tree.extend_from_slice(&build_entry("a/b", 0o040755, 1, 2, &[]));
        tree.extend_from_slice(&build_entry("a/b/c", 0o040755, 1, 3, &[]));
        tree.extend_from_slice(&trailer_entry());
        assert!(traverse_archive(&tree).is_ok());
        // ③ FIFO/设备/套接字类型档。
        let mut special: Vec<u8> = Vec::new();
        special.extend_from_slice(&build_entry("p", 0o010644, 1, 1, &[]));
        special.extend_from_slice(&build_entry("c", 0o020600, 1, 2, &[]));
        special.extend_from_slice(&build_entry("b", 0o060600, 1, 3, &[]));
        special.extend_from_slice(&build_entry("s", 0o140777, 1, 4, &[]));
        special.extend_from_slice(&trailer_entry());
        assert!(traverse_archive(&special).is_ok());
        // ④ 带特殊权限位档（setuid 须并入常规文件类型位 0o100000）。
        let mut suid: Vec<u8> = Vec::new();
        suid.extend_from_slice(&build_entry("m", 0o104755, 1, 1, b"x"));
        suid.extend_from_slice(&trailer_entry());
        assert!(traverse_archive(&suid).is_ok());
    }

    #[test]
    fn health_surface_accounting() {
        let archive = good_archive();
        let mut iter = CpioIterator::from_slice(&archive);
        // 未遍历即问健康：零异常。
        assert!(iter.health().ok);
        // 遍历全量。
        loop {
            match iter.next_entry() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(err) => {
                    assert!(false, "健康面冒烟失败：{}", err.describe());
                    return;
                }
            }
        }
        let health = iter.health();
        assert!(health.ok);
        assert_eq!(health.anomaly_count, 0);
        assert_eq!(health.first_anomaly_code, None);
        assert!(health.events > 0);
        // 腐化后健康面带首异常定位。
        let mut bad = archive.clone();
        bad[0] = b'9';
        let mut iter2 = CpioIterator::from_slice(&bad);
        match iter2.next_entry() {
            Err(err) => {
                assert_eq!(err.code(), V604iCode::MagicMismatch);
                let h2 = iter2.health();
                assert!(!h2.ok);
                assert_eq!(h2.anomaly_count, 1);
                assert_eq!(h2.first_anomaly_code, Some(V604iCode::MagicMismatch.code()));
                assert_eq!(h2.first_anomaly_offset, 0);
                assert!(!h2.first_anomaly_detail.is_empty());
            }
            _ => assert!(false, "腐化档必须报错"),
        }
    }

    #[test]
    fn ino_duplicate_noted_not_rejected() {
        // 相邻两条目同 c_ino：重复账注（不拒收）。
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(&build_entry("x", 0o100644, 1, 7, b"1"));
        archive.extend_from_slice(&build_entry("y", 0o100644, 1, 7, b"2"));
        archive.extend_from_slice(&trailer_entry());
        let report = match traverse_archive(&archive) {
            Ok(r) => r,
            Err(err) => {
                assert!(false, "同 ino 档须全过（只账注）：{}", err.describe());
                return;
            }
        };
        assert_eq!(report.n_entries, 2);
        let mut iter = CpioIterator::from_slice(&archive);
        loop {
            match iter.next_entry() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {
                    assert!(false, "同 ino 遍历须全过");
                    return;
                }
            }
        }
        assert_eq!(iter.ino_duplicates(), 1);
    }

    #[test]
    fn const_layout_assertions() {
        // 编译期账运行面复核（双保险）。
        assert_eq!(HEADER_LEN, 110);
        assert_eq!(NEWC_MAGIC, *b"070701");
        assert_eq!(V604I_SEG_BASE, 0x14B0);
        assert_eq!(V604I_SEG_END, 0x14BF);
        assert_eq!(FIELD_AUDIT_ELEMENTS, 8);
        assert_eq!(AUDIT_ELEMENT_NAMES[AUDIT_INO], "c_ino 登记账");
        assert_eq!(AUDIT_ELEMENT_NAMES[AUDIT_MODE], "c_mode 类型闭集+权限域");
        assert_eq!(AUDIT_ELEMENT_NAMES[AUDIT_CHECK], "c_check 零值核");
    }

    #[test]
    fn version_and_seg_identity() {
        assert_eq!(V604I_VERSION, "v604-cpioiter-v1");
        // 全 16 码 detail 均非空且码值唯一（段内偏移去重）。
        let mut seen = [false; 16];
        let mut i: usize = 0;
        while i < V604iCode::ALL.len() {
            let slot = (V604iCode::ALL[i].code() - V604I_SEG_BASE) as usize;
            assert!(slot < 16, "诊断码落段外");
            assert!(!seen[slot], "诊断码重复");
            seen[slot] = true;
            i += 1;
        }
    }
}




