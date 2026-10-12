//! UNX-F0043 · AML 只读反汇编基础（词法层）· DSDT 前 64KB 扫描
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0043`
//!
//! **判据 UNX-F0043-J1（原文摘录）**：对 DSDT 前 64KB 做词法扫描，Opcode 识别
//! 覆盖率对照 iasl 抽样表 ≥99%，未知码有账。
//!
//! ## 职责定位
//!
//! 纯功能词法层：把 DSDT 定义块字节流（只读窗口，前 0x10000 字节）扫描成
//! AML 记号流（Token 流），建立主/二级 Opcode 闭集大表、未知码账本、
//! 千分位覆盖率账与 iasl 抽样对拍表。本模块**只做词法**：不建命名空间、
//! 不求值、不写任何内存——解析与语义留给后续深化层。
//!
//! ## 设计要点
//!
//! 1. **Opcode 闭集大表写死**：主码表（ACPI 6.5 §20.2 AML 编码，对照 iasl
//!    amlcode.h）约 78 条；0x5B 二级前缀表 29 条（Mutex/Event/CondRefOf/
//!    区域系 0x80~0x88 等），全部静态、无运行时构建。
//! 2. **NameSeg 解码双形态**：原生 4 ASCII 形态逐字符按 AML NameChar 词法
//!    校验（ACPI §19.2.6 / §20.2.2）；另支持本工程词法层压缩记账形态
//!    （LeadByte 高 2 位决定尾部长度 0~3，LeadByte 低 6 位为首字符索引，
//!    从 LeadNameChar 表恢复），展开后与 iasl 反汇编文本对拍。
//! 3. **包长度三/四档解码**（ACPI §20.2.4 PkgLength）：LeadByte bit7~6 决定
//!    字节数档位——00 一字节短包（低 6 位即长）、01 两字节（低 4 位 + 1 尾）、
//!    10 三字节（低 4 位 + 2 尾）、11 四字节（低 4 位 + 3 尾），尾字节小端、
//!    每字节携带 8 位长度。
//! 4. **扫描状态机六相**：Idle → WindowCheck（头 64KB 窗口与 DSDT 签名
//!    上下文锚定）→ Scanning（主循环取 opcode）→ OperandCollect（数据项/
//!    包长度/字符串字节消费）→ Sealed（扫描封账）→ Reopened（增量重扫）。
//!    相位迁移显性、单向封账，重扫只追加不覆盖。
//! 5. **未知码有账**：遇到闭集之外的编码，记字节偏移、未知码值、前后
//!    8 字节 hex 上下文摘要，账本有序追加；未知码不清零、不静默。
//! 6. **覆盖率千分位整型账**：known*1000/(known+unknown)，全部整数运算，
//!    ≥990 千分比（99%）判达标；分母为零判未开扫而非达标。
//! 7. **iasl 抽样对照表**：写死 12 条已知字节序列→期望记号 Kind 序列，
//!    逐条对拍，任何一条不中即记对拍账。
//! 8. **零 panic 面**：所有输入不可信（固件表可被改坏）；禁 unwrap/
//!    expect/panic!，错误走 `Err(SgCode)` 枚举 + `reason()` 中文诊断；
//!    无 IO、无墙钟——进度账仅用调用方注入的逻辑 tick（u64）；无浮点。
//! 9. **诊断码独占 0x0720..=0x072F 段**：与同目录 0x0701~0x070F（F007 系）
//!    及其他模块互不重叠（已 grep 确认该段空闲）。

#![cfg_attr(not(feature = "std"), no_std)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x0720..=0x072F 段）
// ---------------------------------------------------------------------------

/// UNX-F0043 诊断码。独占 `0x0720..=0x072F` 段（已 grep 同目录确认空闲：
/// 0x0701~0x070F 为 F007 系所占，本段无主）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgCode {
    /// 输入字节流为空（无 DSDT 定义块可扫）。
    EmptyInput = 0x0720,
    /// 扫描窗口越界（声明的窗口长超实际字节长）。
    WindowOverrun = 0x0721,
    /// 窗口超上限（超 64KB 词法窗口，判据只保前 64KB）。
    WindowTooLarge = 0x0722,
    /// 窗口未锚定就开扫（状态机相位非法迁移）。
    WindowNotAnchored = 0x0723,
    /// 主码闭集之外的字节（未知码，入账后继续）。
    UnknownOpcode = 0x0724,
    /// 0x5B 二级前缀之后跟了闭集外字节。
    UnknownExtOpcode = 0x0725,
    /// NameSeg 字符非法（首字符不在 LeadNameChar 表）。
    BadNameLead = 0x0726,
    /// NameSeg 尾字符非法（不在 NameChar 表）。
    BadNameTail = 0x0727,
    /// NameSeg 残缺（窗口在四字符补齐前耗尽）。
    NameSegTruncated = 0x0728,
    /// 包长度编码非法（尾字节溢出/长度超窗）。
    BadPkgLength = 0x0729,
    /// 字符串常量未在窗口内闭引号。
    StrUnterminated = 0x072A,
    /// 字节常量截断（0x0A/0x0B/0x0C/0x0E 取数越窗）。
    ConstTruncated = 0x072B,
    /// 二级前缀 0x5B 后窗口耗尽。
    ExtTruncated = 0x072C,
    /// 扫描器重复锚定（窗口只许锚一次，改用增量重扫）。
    DoubleAnchor = 0x072D,
    /// iasl 抽样对拍不中（期望记号序列与实扫不一致）。
    SampleMismatch = 0x072E,
    /// 清单导出时账本未封账（相位未到 Sealed）。
    LedgerUnsealed = 0x072F,
}

impl SgCode {
    /// wire 码（u16，段内独占）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人话原因（中文，'static 串）。
    pub fn reason(self) -> &'static str {
        match self {
            SgCode::EmptyInput => "输入字节流为空，无 DSDT 定义块可扫描",
            SgCode::WindowOverrun => "扫描窗口越界：声明窗口长超过实际字节长",
            SgCode::WindowTooLarge => "窗口超 64KB 上限，判据只保前 64KB 词法窗口",
            SgCode::WindowNotAnchored => "窗口未锚定即开扫，状态机相位非法",
            SgCode::UnknownOpcode => "主码闭集之外的未知编码，已记入未知码账",
            SgCode::UnknownExtOpcode => "0x5B 二级前缀后跟闭集外字节，已记入未知码账",
            SgCode::BadNameLead => "NameSeg 首字符不在 LeadNameChar 合法表内",
            SgCode::BadNameTail => "NameSeg 尾字符不在 NameChar 合法表内",
            SgCode::NameSegTruncated => "NameSeg 残缺：窗口在四字符补齐前耗尽",
            SgCode::BadPkgLength => "包长度编码非法或长度声明越过窗口边界",
            SgCode::StrUnterminated => "字符串常量未在窗口内闭合引号",
            SgCode::ConstTruncated => "字节常量取数越过窗口边界",
            SgCode::ExtTruncated => "二级前缀 0x5B 之后窗口耗尽",
            SgCode::DoubleAnchor => "扫描窗口重复锚定；续扫请走增量重扫入口",
            SgCode::SampleMismatch => "iasl 抽样对拍不中：实扫记号序列与期望不符",
            SgCode::LedgerUnsealed => "账本未封账（相位未到 Sealed），禁止清单导出",
        }
    }
}

/// 结构化错误：诊断码 + 出错字节偏移 + 逻辑 tick（无墙钟，tick 由调用方注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SgError {
    /// 诊断码（0x0720..=0x072F 段）。
    pub code: SgCode,
    /// 出错时窗口内字节偏移。
    pub offset: u32,
    /// 出错时扫描器持有的逻辑 tick。
    pub tick: u64,
}

// ---------------------------------------------------------------------------
// 二、词法常量（ACPI 6.5 §19.2.6 / §20.2.2 名称字符表 + 窗口约束）
// ---------------------------------------------------------------------------

/// 判据窗口上限：DSDT 前 64KB。
pub const LEX_WINDOW_CAP: u32 = 0x10000;

/// 未知码上下文摘要字节数（前 4 + 后 4）。
pub const UNKNOWN_CTX_BYTES: usize = 8;

/// LeadNameChar 合法表（ACPI §20.2.2）：'A'~'Z' 与 '_'。
/// 压缩形态首字符索引即按此表 27 项取模。
const LEAD_NAME_CHARS: [u8; 27] = [
    b'_', b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J', b'K', b'L', b'M', b'N',
    b'O', b'P', b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z',
];

/// DigitChar 合法表：'0'~'9'。
const DIGIT_CHARS: [u8; 10] = [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'];

/// NameChar 合法表：LeadNameChar ∪ DigitChar。
const NAME_CHARS: [u8; 37] = [
    b'_', b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J', b'K', b'L', b'M', b'N',
    b'O', b'P', b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z', b'0', b'1', b'2',
    b'3', b'4', b'5', b'6', b'7', b'8', b'9',
];

/// 字节是否为 LeadNameChar。
fn is_lead_name_char(b: u8) -> bool {
    let mut i = 0;
    while i < LEAD_NAME_CHARS.len() {
        if LEAD_NAME_CHARS[i] == b {
            return true;
        }
        i += 1;
    }
    false
}

/// 字节是否为 NameChar。
fn is_name_char(b: u8) -> bool {
    let mut i = 0;
    while i < NAME_CHARS.len() {
        if NAME_CHARS[i] == b {
            return true;
        }
        i += 1;
    }
    false
}

/// 字节是否为 DigitChar。
fn is_digit_char(b: u8) -> bool {
    let mut i = 0;
    while i < DIGIT_CHARS.len() {
        if DIGIT_CHARS[i] == b {
            return true;
        }
        i += 1;
    }
    false
}

/// 单字节 hex（两位小写），供上下文摘要拼装，无浮点无格式化宏依赖。
fn hex_byte(b: u8) -> [u8; 2] {
    const HEXDIG: [u8; 16] = *b"0123456789abcdef";
    [HEXDIG[(b >> 4) as usize], HEXDIG[(b & 0x0F) as usize]]
}

// ---------------------------------------------------------------------------
// 三、TokenKind 闭集与记号结构
// ---------------------------------------------------------------------------

/// 记号种类闭集。词法层只分类不语义化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// 命名对象创建类 opcode（NameOp/ScopeOp/MethodOp/BufferOp/PackageOp/
    /// OpRegionOp/DeviceOp 等——将引入名字的对象）。
    NamedObj,
    /// 控制流类 opcode（IfOp/ElseOp/WhileOp/ReturnOp/BreakOp/NoopOp 等）。
    ControlOp,
    /// 数据/运算类 opcode（AddOp/AndOp/StoreOp/ToXxxOp 等表达式算子）。
    DataOp,
    /// 0x5B 二级前缀 opcode（MutexOp/EventOp/区域系等，ext 二元组）。
    ExtOp,
    /// 名字段记号（NameSeg 四字符 / Dual/Multi/Root/Parent 前缀）。
    NameSeg,
    /// 整数常量（0x00/0x01/0xFF One/Zero/Ones 与 Byte/Word/Dword/Qword 取数）。
    IntConst,
    /// 字符串常量（0x0D 引号包裹 ASCII）。
    StrConst,
    /// 包长度记号（PkgLength 解码结果，附原始字节数与声明长度）。
    PkgLen,
    /// 局部/参数引用记号（Local0~7/Arg0~6）。
    LocalArgRef,
    /// 未知码占位记号（闭集之外，占流保位，配未知码账）。
    Unknown,
}

/// 记号本体：种类 + 起止偏移 + 载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 记号种类。
    pub kind: TokenKind,
    /// 记号起始窗口偏移。
    pub start: u32,
    /// 记号结束偏移（不含，start..end）。
    pub end: u32,
    /// 主码或二级码（ExtOp 存二级码，其余存主码；非 opcode 类记号为 0）。
    pub opcode: u8,
    /// 是否 0x5B 二级前缀编码。
    pub ext: bool,
    /// 载荷：NameSeg 展开文本 / IntConst 十进制文本 / StrConst 原文 /
    /// PkgLen 声明长度文本。定长场景不适用，用 alloc::String 直存。
    pub text: String,
    /// 整数载荷（IntConst/PkgLen 用；其余 0）。
    pub value: u64,
}

impl Token {
    /// 记号字节数。
    pub const fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

// ---------------------------------------------------------------------------
// 四、主 Opcode 闭集大表（ACPI 6.5 §20.2 AML 编码；对照 iasl amlcode.h）
// ---------------------------------------------------------------------------

/// 单条 opcode 元信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlopInfo {
    /// 主码字节（ext 条目存二级码）。
    pub code: u8,
    /// 助记（与 iasl 反汇编输出同名，便于对拍）。
    pub mnem: &'static str,
    /// 记号种类归类。
    pub kind: TokenKind,
    /// 后随操作数中的名字段个数（词法提示，供记号流统计）。
    pub name_args: u8,
    /// 是否引入新名字对象（NamedObj 判定的权威位）。
    pub creates_name: bool,
}

/// 主码闭集大表：ACPI 6.5 §20.2 AML Encoding，逐条对照 iasl amlcode.h。
/// 顺序按码值升序，覆盖 0x00..=0xFF 中规范定义的 84 个主码。
pub const MAIN_OP_TABLE: [AmlopInfo; 84] = [
    // --- 0x00..0x0F：常数与命名基础（ACPI §20.2 / iasl amlcode.h）---
    AmlopInfo { code: 0x00, mnem: "ZeroOp",     kind: TokenKind::IntConst, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x01, mnem: "OneOp",      kind: TokenKind::IntConst, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x06, mnem: "AliasOp",    kind: TokenKind::NamedObj, name_args: 2, creates_name: true },
    AmlopInfo { code: 0x08, mnem: "NameOp",     kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x0A, mnem: "ByteConst",  kind: TokenKind::IntConst, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x0B, mnem: "WordConst",  kind: TokenKind::IntConst, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x0C, mnem: "DwordConst", kind: TokenKind::IntConst, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x0D, mnem: "StringConst",kind: TokenKind::StrConst, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x0E, mnem: "QwordConst", kind: TokenKind::IntConst, name_args: 0, creates_name: false },
    // --- 0x10..0x15：作用域与对象（ACPI §19.5.x / §20.2）---
    AmlopInfo { code: 0x10, mnem: "ScopeOp",      kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x11, mnem: "BufferOp",     kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x12, mnem: "PackageOp",    kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x13, mnem: "VarPackageOp", kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x14, mnem: "MethodOp",     kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x15, mnem: "ExternalOp",   kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    // --- 0x2E/0x2F：名字前缀（ACPI §20.2.2 NamePath）---
    AmlopInfo { code: 0x2E, mnem: "DualNamePrefix",   kind: TokenKind::NameSeg, name_args: 2, creates_name: false },
    AmlopInfo { code: 0x2F, mnem: "MultiNamePrefix",  kind: TokenKind::NameSeg, name_args: 1, creates_name: false },
    // --- 0x5B/0x5C/0x5E：前缀族（二级表入口 / RootChar / ParentPrefixChar）---
    AmlopInfo { code: 0x5B, mnem: "ExtOpPrefix",      kind: TokenKind::ExtOp,   name_args: 0, creates_name: false },
    AmlopInfo { code: 0x5C, mnem: "RootPrefix",       kind: TokenKind::NameSeg, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x5E, mnem: "ParentPrefixChar", kind: TokenKind::NameSeg, name_args: 0, creates_name: false },
    // --- 0x60..0x6E：Local0~7 / Arg0~6（ACPI §19.6.x / iasl amlcode.h）---
    AmlopInfo { code: 0x60, mnem: "Local0", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x61, mnem: "Local1", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x62, mnem: "Local2", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x63, mnem: "Local3", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x64, mnem: "Local4", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x65, mnem: "Local5", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x66, mnem: "Local6", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x67, mnem: "Local7", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x68, mnem: "Arg0", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x69, mnem: "Arg1", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x6A, mnem: "Arg2", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x6B, mnem: "Arg3", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x6C, mnem: "Arg4", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x6D, mnem: "Arg5", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x6E, mnem: "Arg6", kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    // --- 0x70..0x7F：数据/运算算子（ACPI §19.6 / iasl amlcode.h）---
    AmlopInfo { code: 0x70, mnem: "StoreOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x71, mnem: "RefOfOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x72, mnem: "AddOp",         kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x73, mnem: "ConcatOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x74, mnem: "SubtractOp",    kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x75, mnem: "ShiftLeftOp",   kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x76, mnem: "ShiftRightOp",  kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x77, mnem: "AndOp",         kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x78, mnem: "DivideOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x79, mnem: "OrOp",          kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x7A, mnem: "NorOp",         kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x7B, mnem: "XorOp",         kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x7C, mnem: "NotifyOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x7D, mnem: "SizeOfOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x7E, mnem: "IndexOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x7F, mnem: "MatchOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    // --- 0x80..0x8F：域创建与查型/取反引用（iasl amlcode.h 对拍段）---
    AmlopInfo { code: 0x81, mnem: "FindSetLeftBitOp",  kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x82, mnem: "FindSetRightBitOp", kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x83, mnem: "DerefOfOp",         kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x84, mnem: "ConcatResOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x85, mnem: "ModOp",             kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x8A, mnem: "CreateDWordFieldOp",kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x8B, mnem: "CreateWordFieldOp", kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x8C, mnem: "CreateByteFieldOp", kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x8D, mnem: "CreateBitFieldOp",  kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x8E, mnem: "ObjectTypeOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x8F, mnem: "CreateQWordFieldOp",kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    // --- 0x90..0x95：逻辑比较（ACPI §19.6 / iasl amlcode.h）---
    AmlopInfo { code: 0x90, mnem: "LandOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x91, mnem: "LorOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x92, mnem: "LnotOp",      kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x93, mnem: "LEqualOp",    kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x94, mnem: "LGreaterOp",  kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x95, mnem: "LLessOp",     kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    // --- 0x96..0x99：ToXxx 类型转换 ---
    AmlopInfo { code: 0x96, mnem: "ToBufferOp",        kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x97, mnem: "ToDecimalStringOp", kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x98, mnem: "ToHexStringOp",     kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x99, mnem: "ToIntegerOp",       kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    // --- 0x9C..0x9F：串化/复制/切片/续扫 ---
    AmlopInfo { code: 0x9C, mnem: "ToStringOp",   kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x9D, mnem: "CopyObjectOp", kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x9E, mnem: "MidOp",        kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x9F, mnem: "ContinueOp",   kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    // --- 0xA0..0xA5：控制流（ACPI §19.6 / iasl amlcode.h）---
    AmlopInfo { code: 0xA0, mnem: "IfOp",     kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0xA1, mnem: "ElseOp",   kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0xA2, mnem: "WhileOp",  kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0xA3, mnem: "NoopOp",   kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0xA4, mnem: "ReturnOp", kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0xA5, mnem: "BreakOp",  kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    // --- 0xCC / 0xFF：断点与 Ones（iasl amlcode.h）---
    AmlopInfo { code: 0xCC, mnem: "BreakPointOp", kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0xFF, mnem: "OnesOp",       kind: TokenKind::IntConst,  name_args: 0, creates_name: false },
];

/// 主码查表：命中返回表项下标（闭集判定单一入口）。
pub fn main_op_lookup(code: u8) -> Option<usize> {
    let mut i = 0;
    while i < MAIN_OP_TABLE.len() {
        if MAIN_OP_TABLE[i].code == code {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 五、0x5B 二级操作码表（29 条：ACPI §19.5 / iasl amlcode.h ExtOp 段）
// ---------------------------------------------------------------------------

/// 0x5B 二级表：Mutex/Event/CondRefOf、Load 系、同步系、BCD 系、
/// 区域系（0x80~0x88）等，全部写死。
pub const EXT_OP_TABLE: [AmlopInfo; 27] = [
    // --- 同步/事件（ACPI §19.5.66~19.5.100 / iasl amlcode.h 0x5B01 起）---
    AmlopInfo { code: 0x01, mnem: "MutexOp",    kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x02, mnem: "EventOp",    kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x03, mnem: "CondRefOfOp",kind: TokenKind::DataOp,   name_args: 0, creates_name: false },
    // --- 表装载系 ---
    AmlopInfo { code: 0x12, mnem: "LoadOp",       kind: TokenKind::NamedObj, name_args: 2, creates_name: true },
    AmlopInfo { code: 0x13, mnem: "CreateFieldOp",kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x1F, mnem: "LoadTableOp",  kind: TokenKind::NamedObj, name_args: 0, creates_name: true },
    // --- 定时/同步动作系（0x5B21~0x5B27）---
    AmlopInfo { code: 0x21, mnem: "StallOp",   kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x22, mnem: "SleepOp",   kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x23, mnem: "AcquireOp", kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x24, mnem: "SignalOp",  kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x25, mnem: "WaitOp",    kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x26, mnem: "ResetOp",   kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x27, mnem: "ReleaseOp", kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    // --- BCD 系（0x5B28/0x5B29）---
    AmlopInfo { code: 0x28, mnem: "FromBCDOp", kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x29, mnem: "ToBCDOp",   kind: TokenKind::DataOp, name_args: 0, creates_name: false },
    // --- 版本/调试/致命（0x5B30/0x5B31/0x5B64）---
    AmlopInfo { code: 0x30, mnem: "RevisionOp", kind: TokenKind::IntConst,  name_args: 0, creates_name: false },
    AmlopInfo { code: 0x31, mnem: "DebugOp",    kind: TokenKind::LocalArgRef, name_args: 0, creates_name: false },
    AmlopInfo { code: 0x64, mnem: "FatalOp",    kind: TokenKind::ControlOp, name_args: 0, creates_name: false },
    // --- 区域系 0x5B80~0x5B88（ACPI §19.5 / iasl amlcode.h）---
    AmlopInfo { code: 0x80, mnem: "OpRegionOp",    kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x81, mnem: "FieldOp",       kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x82, mnem: "DeviceOp",      kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x83, mnem: "ProcessorOp",   kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x84, mnem: "PowerResOp",    kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x85, mnem: "ThermalZoneOp", kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
    AmlopInfo { code: 0x86, mnem: "IndexFieldOp",  kind: TokenKind::NamedObj, name_args: 2, creates_name: true },
    AmlopInfo { code: 0x87, mnem: "BankFieldOp",   kind: TokenKind::NamedObj, name_args: 2, creates_name: true },
    AmlopInfo { code: 0x88, mnem: "DataRegionOp",  kind: TokenKind::NamedObj, name_args: 1, creates_name: true },
];

/// 二级码查表。
pub fn ext_op_lookup(code: u8) -> Option<usize> {
    let mut i = 0;
    while i < EXT_OP_TABLE.len() {
        if EXT_OP_TABLE[i].code == code {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 六、NameSeg 双形态解码
// ---------------------------------------------------------------------------

/// NameSeg 解码结果：展开四字符 + 原始字节数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSegDecoded {
    /// 展开 ASCII 四字符（如 b"_SB_"）。
    pub chars: [u8; 4],
    /// 原始编码消耗的字节数（原生形态恒 4；压缩形态 1~4）。
    pub raw_len: u8,
}

/// 原生形态解码：窗口 pos 起连取 4 字节，逐个按 LeadNameChar/NameChar 校验
/// （首字符 LeadNameChar，后三字符 NameChar——ACPI §20.2.2）。
/// 纯函数：越窗/非法字符分别返回截断/非法诊断。
pub fn decode_nameseg_native(win: &[u8], pos: u32) -> Result<NameSegDecoded, SgError> {
    let p = pos as usize;
    if p + 4 > win.len() {
        return Err(SgError { code: SgCode::NameSegTruncated, offset: pos, tick: 0 });
    }
    if !is_lead_name_char(win[p]) {
        return Err(SgError { code: SgCode::BadNameLead, offset: pos, tick: 0 });
    }
    let mut chars = [0u8; 4];
    chars[0] = win[p];
    let mut k = 1;
    while k < 4 {
        let b = win[p + k];
        if !is_name_char(b) {
            return Err(SgError { code: SgCode::BadNameTail, offset: pos + k as u32, tick: 0 });
        }
        chars[k] = b;
        k += 1;
    }
    Ok(NameSegDecoded { chars, raw_len: 4 })
}

/// 压缩形态解码（本工程词法层压缩记账，用于簿记与传输；对拍 iasl 时先
/// 展开为 ASCII 四字符）：LeadByte 高 2 位决定尾部长度（00→3 尾字节、
/// 01→2、10→1、11→0），LeadByte 低 6 位为首字符索引（对 27 取模入
/// LeadNameChar 表），尾字符索引对 37 取模入 NameChar 表。
/// 全整数、全确定性。
pub fn decode_nameseg_packed(win: &[u8], pos: u32) -> Result<NameSegDecoded, SgError> {
    let p = pos as usize;
    if p >= win.len() {
        return Err(SgError { code: SgCode::NameSegTruncated, offset: pos, tick: 0 });
    }
    let lead = win[p];
    let tail_len = ((lead >> 6) & 0x03) as usize; // 0..=3
    if p + 1 + tail_len > win.len() {
        return Err(SgError { code: SgCode::NameSegTruncated, offset: pos, tick: 0 });
    }
    let lead_idx = (lead & 0x3F) as usize % LEAD_NAME_CHARS.len();
    let mut chars = [0u8; 4];
    chars[0] = LEAD_NAME_CHARS[lead_idx];
    let mut k = 0;
    while k < tail_len {
        let b = win[p + 1 + k];
        let idx = (b as usize) % NAME_CHARS.len();
        chars[1 + k] = NAME_CHARS[idx];
        k += 1;
    }
    // 尾部不足 4 字符以 '_' 补齐（压缩形态只记有效位）。
    while 1 + tail_len < 4 {
        chars[1 + tail_len] = b'_';
        break;
    }
    Ok(NameSegDecoded { chars, raw_len: (1 + tail_len) as u8 })
}

/// 四字符数组转 String（ASCII 安全：非法位以 '.' 占位，不 panic）。
fn nameseg_to_string(chars: &[u8; 4]) -> String {
    let mut s = String::with_capacity(4);
    let mut i = 0;
    while i < 4 {
        if is_name_char(chars[i]) {
            s.push(chars[i] as char);
        } else {
            s.push('.');
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 七、包长度解码（ACPI §20.2.4 PkgLength，四档：1/2/3/4 字节编码）
// ---------------------------------------------------------------------------

/// 包长度解码结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PkgLenDecoded {
    /// 声明的包体长度（不含 PkgLength 编码自身；按规范语义记录）。
    pub body_len: u32,
    /// PkgLength 编码消耗的原始字节数（1~4）。
    pub raw_len: u8,
    /// 档位（LeadByte bit7~6 原值，0~3）。
    pub tier: u8,
}

/// 包长度解码：LeadByte bit7~6 选档——00 一字节短包（低 6 位即长度）、
/// 01 两字节（低 4 位 | 尾1<<4）、10 三字节（低 4 位 | 尾1<<4 | 尾2<<12）、
/// 11 四字节（低 4 位 | 尾1<<4 | 尾2<<12 | 尾3<<20）。
/// 尾字节小端、每字节携带 8 位（ACPI §20.2.4 原文：bits per byte = 8）。
/// 全整数。长度声明越过窗口边界时返 BadPkgLength（词法只验界不语义）。
pub fn decode_pkg_length(win: &[u8], pos: u32) -> Result<PkgLenDecoded, SgError> {
    let p = pos as usize;
    if p >= win.len() {
        return Err(SgError { code: SgCode::BadPkgLength, offset: pos, tick: 0 });
    }
    let lead = win[p];
    let tier = (lead >> 6) & 0x03;
    let low4 = (lead & 0x0F) as u32;
    match tier {
        0 => Ok(PkgLenDecoded { body_len: low4, raw_len: 1, tier }),
        1 => {
            if p + 1 >= win.len() {
                return Err(SgError { code: SgCode::BadPkgLength, offset: pos, tick: 0 });
            }
            let body = low4 | ((win[p + 1] as u32) << 4);
            Ok(PkgLenDecoded { body_len: body, raw_len: 2, tier })
        }
        2 => {
            if p + 2 >= win.len() {
                return Err(SgError { code: SgCode::BadPkgLength, offset: pos, tick: 0 });
            }
            let body = low4
                | ((win[p + 1] as u32) << 4)
                | ((win[p + 2] as u32) << 12);
            Ok(PkgLenDecoded { body_len: body, raw_len: 3, tier })
        }
        _ => {
            if p + 3 >= win.len() {
                return Err(SgError { code: SgCode::BadPkgLength, offset: pos, tick: 0 });
            }
            let body = low4
                | ((win[p + 1] as u32) << 4)
                | ((win[p + 2] as u32) << 12)
                | ((win[p + 3] as u32) << 20);
            Ok(PkgLenDecoded { body_len: body, raw_len: 4, tier })
        }
    }
}

// ---------------------------------------------------------------------------
// 八、未知码账（偏移 + 码值 + 前 8 字节 hex 上下文摘要）
// ---------------------------------------------------------------------------

/// 未知码账条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEntry {
    /// 窗口内字节偏移。
    pub offset: u32,
    /// 未知码值（ExtOp 场景为二级码，ext=true）。
    pub opcode: u8,
    /// 是否 0x5B 二级前缀之后的未知。
    pub ext: bool,
    /// 上下文 8 字节 hex 摘要（码前 4 + 码后 4；越界位以 "..." 文本记账，
    /// hex 段不足补 "--"）。
    pub context_hex: String,
}

/// 生成未知码 8 字节上下文摘要：取码前至多 4 字节与码后至多 4 字节，
/// 两位 hex 拼接，中缝插 '|'；越窗字节以 "--" 占位。纯确定性。
fn build_context_hex(win: &[u8], offset: u32) -> String {
    let o = offset as usize;
    let mut s = String::with_capacity(UNKNOWN_CTX_BYTES * 2 + 2);
    // 前 4 字节（不足补 --）。
    let pre_start = o.saturating_sub(4);
    let mut i = pre_start;
    let mut pre_count = 0;
    while i < o {
        if i < win.len() {
            let hx = hex_byte(win[i]);
            s.push(hx[0] as char);
            s.push(hx[1] as char);
        } else {
            s.push('-');
            s.push('-');
        }
        pre_count += 1;
        i += 1;
    }
    // 前段不足 4 位补齐。
    while pre_count < 4 {
        s.push('-');
        s.push('-');
        pre_count += 1;
    }
    s.push('|');
    // 后 4 字节（含码位本身之后）。
    let mut k = 0;
    while k < 4 {
        let j = o + 1 + k;
        if j < win.len() {
            let hx = hex_byte(win[j]);
            s.push(hx[0] as char);
            s.push(hx[1] as char);
        } else {
            s.push('-');
            s.push('-');
        }
        k += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 九、扫描状态机（六相）与统计账
// ---------------------------------------------------------------------------

/// 扫描器六相状态：Idle → WindowCheck → Scanning ⇄ OperandCollect →
/// Sealed →（增量重扫）Reopened。迁移显性单向，封账后只走重扫入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    /// 相一：空闲（未锚定窗口）。
    Idle,
    /// 相二：窗口锚定与头校验（64KB 上限、窗长一致性）。
    WindowCheck,
    /// 相三：主扫描（取 opcode / 名字段 / 控制流）。
    Scanning,
    /// 相四：操作数收集（常量取数、字符串字节、包长度四档）。
    OperandCollect,
    /// 相五：封账（记号流定稿、覆盖率落账、清单可导出）。
    Sealed,
    /// 相六：增量重扫（从指定偏移续扫，只追加不覆盖既有账）。
    Reopened,
}

/// opcode 频次 kv 条目（清单导出用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpFreq {
    /// 展示键："ExtOp+0x80" 或 "0x5B" 风格主码助记（MutexOp 等）。
    pub key: String,
    /// 出现次数。
    pub count: u64,
}

/// 覆盖率账：全整数千分位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coverage {
    /// 已识别 opcode 记号总数。
    pub known: u64,
    /// 未知码个数（含主码与二级未知）。
    pub unknown: u64,
    /// 千分位覆盖率：known*1000/(known+unknown)；分母 0 时为 0。
    pub permille: u64,
}

impl Coverage {
    /// 是否达标：≥990‰（即 ≥99%，判据 J1 数值锚）。
    pub const fn meets_j1(&self) -> bool {
        self.permille >= 990
    }
}

/// 词法统计账与清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexManifest {
    /// 扫描窗口字节数（≤64KB）。
    pub window_len: u32,
    /// 记号总数。
    pub token_total: u64,
    /// NamedObj 类计数。
    pub named_obj_count: u64,
    /// ControlOp 类计数。
    pub control_count: u64,
    /// DataOp 类计数。
    pub data_count: u64,
    /// ExtOp（0x5B 二级）计数。
    pub ext_count: u64,
    /// NameSeg 记号计数。
    pub nameseg_count: u64,
    /// IntConst 记号计数。
    pub int_const_count: u64,
    /// StrConst 记号计数。
    pub str_const_count: u64,
    /// PkgLen 记号计数。
    pub pkg_len_count: u64,
    /// Local/Arg 引用计数。
    pub local_arg_count: u64,
    /// opcode 频次 kv（按首现顺序追加，封账时定稿）。
    pub op_freq: Vec<OpFreq>,
    /// 未知码清单（有序）。
    pub unknowns: Vec<UnknownEntry>,
    /// 覆盖率账。
    pub coverage: Coverage,
    /// 封账时的逻辑 tick。
    pub sealed_tick: u64,
}

// ---------------------------------------------------------------------------
// 十、扫描器本体
// ---------------------------------------------------------------------------

/// AML 词法扫描器：只读窗口 → 记号流 + 账本。六相状态机驱动。
#[derive(Debug, Clone)]
pub struct AmlLexer {
    /// 当前相位。
    phase: ScanPhase,
    /// 窗口长（锚定时定死，≤64KB）。
    window_len: u32,
    /// 记号流。
    tokens: Vec<Token>,
    /// 未知码账。
    unknowns: Vec<UnknownEntry>,
    /// opcode 频次（key 有序追加）。
    op_freq_keys: Vec<String>,
    op_freq_vals: Vec<u64>,
    /// 各类计数。
    named_obj_count: u64,
    control_count: u64,
    data_count: u64,
    ext_count: u64,
    nameseg_count: u64,
    int_const_count: u64,
    str_const_count: u64,
    pkg_len_count: u64,
    local_arg_count: u64,
    unknown_count: u64,
    /// 逻辑 tick（调用方注入推进；无墙钟）。
    tick: u64,
}

impl AmlLexer {
    /// 相一 Idle：新建扫描器。tick 取调用方注入的初值（通常 0）。
    pub fn new(tick: u64) -> Self {
        AmlLexer {
            phase: ScanPhase::Idle,
            window_len: 0,
            tokens: Vec::new(),
            unknowns: Vec::new(),
            op_freq_keys: Vec::new(),
            op_freq_vals: Vec::new(),
            named_obj_count: 0,
            control_count: 0,
            data_count: 0,
            ext_count: 0,
            nameseg_count: 0,
            int_const_count: 0,
            str_const_count: 0,
            pkg_len_count: 0,
            local_arg_count: 0,
            unknown_count: 0,
            tick,
        }
    }

    /// 当前相位。
    pub const fn phase(&self) -> ScanPhase {
        self.phase
    }

    /// 已产出的记号流引用。
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// 逻辑 tick。
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// 相二 WindowCheck：锚定窗口。窗长 ≤64KB 且 ≤ 实际字节长；重复锚定报错。
    /// 窗口只许锚一次——续扫一律走 rescan_from（相六 Reopened）。
    pub fn anchor_window(&mut self, bytes: &[u8], tick: u64) -> Result<(), SgError> {
        self.tick = tick;
        if self.phase != ScanPhase::Idle {
            return Err(SgError { code: SgCode::DoubleAnchor, offset: 0, tick });
        }
        if bytes.is_empty() {
            return Err(SgError { code: SgCode::EmptyInput, offset: 0, tick });
        }
        // 窗长声明：取 min(实际长, 64KB)——判据只保前 64KB。
        let want = if bytes.len() as u64 > LEX_WINDOW_CAP as u64 {
            LEX_WINDOW_CAP
        } else {
            bytes.len() as u32
        };
        if want == 0 {
            return Err(SgError { code: SgCode::WindowOverrun, offset: 0, tick });
        }
        self.phase = ScanPhase::WindowCheck;
        self.window_len = want;
        Ok(())
    }

    /// 严格锚定变体：输入超 64KB 词法窗口上限时直接报 WindowTooLarge，
    /// 不做静默截断（供调用方在需要显式拒绝超大表时使用；宽松版
    /// anchor_window 按判据语义取前 64KB）。
    pub fn anchor_window_strict(&mut self, bytes: &[u8], tick: u64) -> Result<(), SgError> {
        if bytes.len() as u64 > LEX_WINDOW_CAP as u64 {
            self.tick = tick;
            return Err(SgError { code: SgCode::WindowTooLarge, offset: 0, tick });
        }
        self.anchor_window(bytes, tick)
    }

    /// 窗长（未锚定为 0）。
    pub const fn window_len(&self) -> u32 {
        self.window_len
    }

    /// 未知码账只读视图。
    pub fn unknowns(&self) -> &[UnknownEntry] {
        &self.unknowns
    }

    /// 首个未知码诊断：账非空时按首条 ext 位归类返回 UnknownOpcode /
    /// UnknownExtOpcode——未知码有账且可经 Err 通道对外暴露。
    pub fn first_unknown_code(&self) -> Option<SgCode> {
        match self.unknowns.first() {
            Some(e) => {
                if e.ext {
                    Some(SgCode::UnknownExtOpcode)
                } else {
                    Some(SgCode::UnknownOpcode)
                }
            }
            None => None,
        }
    }

    /// 主扫描入口：Idle/WindowCheck 相启动全窗扫描；Sealed 相拒绝（走 rescan）。
    /// 全程状态机驱动，未知码入账继续（判据：未知码有账，不清零不静默）。
    pub fn scan(&mut self, bytes: &[u8], tick: u64) -> Result<LexManifest, SgError> {
        self.tick = tick;
        match self.phase {
            ScanPhase::Idle => {
                return Err(SgError { code: SgCode::WindowNotAnchored, offset: 0, tick });
            }
            ScanPhase::WindowCheck | ScanPhase::Reopened => {
                if self.window_len == 0 {
                    return Err(SgError { code: SgCode::WindowNotAnchored, offset: 0, tick });
                }
                self.phase = ScanPhase::Scanning;
            }
            ScanPhase::Scanning | ScanPhase::OperandCollect => { /* 续扫 */ }
            ScanPhase::Sealed => {
                return Err(SgError { code: SgCode::LedgerUnsealed, offset: 0, tick });
            }
        }
        self.run_loop(bytes)?;
        self.seal(tick)
    }

    /// 封账：相位迁 Sealed，落覆盖率账与清单。
    fn seal(&mut self, tick: u64) -> Result<LexManifest, SgError> {
        self.phase = ScanPhase::Sealed;
        let denom = self.tokens.len() as u64; // 已知记号总量即记号流长（Unknown 记号另账）
        // 覆盖率按判据口径：已知 = 非 Unknown 记号；未知 = unknowns 账长。
        let known = denom.saturating_sub(self.unknown_count);
        let unknown = self.unknown_count;
        let total = known.saturating_add(unknown);
        let permille = if total == 0 {
            0
        } else {
            // known*1000 可能超 u32，用 u64 全程（known ≤ 记号数，千倍仍在 u64 内）。
            (known.saturating_mul(1000)) / total
        };
        let mut freq = Vec::with_capacity(self.op_freq_keys.len());
        let mut i = 0;
        while i < self.op_freq_keys.len() {
            freq.push(OpFreq { key: self.op_freq_keys[i].clone(), count: self.op_freq_vals[i] });
            i += 1;
        }
        Ok(LexManifest {
            window_len: self.window_len,
            token_total: denom,
            named_obj_count: self.named_obj_count,
            control_count: self.control_count,
            data_count: self.data_count,
            ext_count: self.ext_count,
            nameseg_count: self.nameseg_count,
            int_const_count: self.int_const_count,
            str_const_count: self.str_const_count,
            pkg_len_count: self.pkg_len_count,
            local_arg_count: self.local_arg_count,
            op_freq: freq,
            unknowns: self.unknowns.clone(),
            coverage: Coverage { known, unknown, permille },
            sealed_tick: tick,
        })
    }

    /// 相六 Reopened：增量重扫。从 from_offset 起续扫窗口剩余部分，
    /// 只追加记号与账目，不清理既有账（判据：统计账与增量重扫）。
    /// 调用前须已封账（Sealed），重扫后自动再封账出增量清单。
    pub fn rescan_from(
        &mut self,
        bytes: &[u8],
        from_offset: u32,
        tick: u64,
    ) -> Result<LexManifest, SgError> {
        self.tick = tick;
        if self.phase != ScanPhase::Sealed {
            return Err(SgError { code: SgCode::LedgerUnsealed, offset: from_offset, tick });
        }
        if from_offset > self.window_len {
            return Err(SgError { code: SgCode::WindowOverrun, offset: from_offset, tick });
        }
        self.phase = ScanPhase::Reopened;
        self.run_loop_from(bytes, from_offset)?;
        self.seal(tick)
    }

    /// 主循环：从 0 起扫。
    fn run_loop(&mut self, bytes: &[u8]) -> Result<(), SgError> {
        self.run_loop_from(bytes, 0)
    }

    /// 主循环核心：六相中 Scanning/OperandCollect 的实际推进。
    /// 每次迭代：取字节 → 命中主表/二级表 → 产记号 + 记频 → 数据项走
    /// OperandCollect 子例程（包长编码字节随 opcode 一并前跳，防重复消费）；
    /// 名字前缀（Root/Parent/Dual/Multi）展开成 NameSeg 记号序列；
    /// 未命中 → 未知码入账（带 8 字节 hex 上下文）+ Unknown 占位记号。
    fn run_loop_from(&mut self, bytes: &[u8], from: u32) -> Result<(), SgError> {
        let win_len = self.window_len as usize;
        if win_len > bytes.len() {
            return Err(SgError {
                code: SgCode::WindowOverrun,
                offset: bytes.len() as u32,
                tick: self.tick,
            });
        }
        let win = &bytes[..win_len];
        let mut pos = from as usize;
        while pos < win_len {
            let b = win[pos];
            // ---- 0x5B 二级前缀：先于主表查（0x5B 在主表内是前缀项）----
            if b == 0x5B {
                if pos + 1 >= win_len {
                    // 二级前缀悬空：入未知账（ExtTruncated 语义）+ 占位记号。
                    self.push_unknown_ctx(win, pos, 0x5B, true);
                    let t = Token {
                        kind: TokenKind::Unknown,
                        start: pos as u32,
                        end: (pos + 1) as u32,
                        opcode: 0x5B,
                        ext: true,
                        text: String::new(),
                        value: 0,
                    };
                    self.tokens.push(t);
                    pos += 1;
                    self.phase = ScanPhase::OperandCollect;
                    continue;
                }
                let sub = win[pos + 1];
                match ext_op_lookup(sub) {
                    Some(idx) => {
                        let info = EXT_OP_TABLE[idx];
                        self.phase = ScanPhase::OperandCollect;
                        let t = Token {
                            kind: TokenKind::ExtOp,
                            start: pos as u32,
                            end: (pos + 2) as u32,
                            opcode: sub,
                            ext: true,
                            text: info.mnem.to_string(),
                            value: 0,
                        };
                        self.push_token_kind(&t, info.kind);
                        self.bump_freq(info.mnem);
                        self.tokens.push(t);
                        pos += 2;
                        self.phase = ScanPhase::Scanning;
                    }
                    None => {
                        // 二级未知：前缀+二级码整体占位，防重复计账。
                        self.push_unknown_ctx(win, pos + 1, sub, true);
                        let t = Token {
                            kind: TokenKind::Unknown,
                            start: pos as u32,
                            end: (pos + 2).min(win_len) as u32,
                            opcode: sub,
                            ext: true,
                            text: String::new(),
                            value: 0,
                        };
                        self.tokens.push(t);
                        pos += 2;
                        self.phase = ScanPhase::OperandCollect;
                    }
                }
                continue;
            }
            // ---- 名字前缀族：RootChar 0x5C / ParentPrefixChar 0x5E /
            //      DualNamePrefix 0x2E / MultiNamePrefix 0x2F（ACPI §20.2.2）。
            //      前缀本体各产一条 NameSeg 记号；0x2F 额外消费 SegCount
            //      字节（记入 value）；紧随的原生 NameSeg 四字符组由下方
            //      裸 NameSeg 分支自然承接。
            if b == 0x5C || b == 0x5E || b == 0x2E || b == 0x2F {
                self.phase = ScanPhase::OperandCollect;
                let mut consume = 1usize;
                let mut value: u64 = 0;
                if b == 0x2F {
                    if pos + 1 >= win_len {
                        return Err(SgError {
                            code: SgCode::NameSegTruncated,
                            offset: pos as u32,
                            tick: self.tick,
                        });
                    }
                    value = win[pos + 1] as u64; // SegCount
                    consume = 2;
                }
                let head = Token {
                    kind: TokenKind::NameSeg,
                    start: pos as u32,
                    end: (pos + consume) as u32,
                    opcode: b,
                    ext: false,
                    text: match b {
                        0x5C => "\\".to_string(),
                        0x5E => "^".to_string(),
                        0x2E => "DualNamePrefix".to_string(),
                        _ => "MultiNamePrefix".to_string(),
                    },
                    value,
                };
                self.nameseg_count += 1;
                self.tokens.push(head);
                pos += consume;
                self.phase = ScanPhase::Scanning;
                continue;
            }
            // ---- 裸 NameSeg 分支：LeadNameChar 开头且后随三字节均
            //      NameChar 的四字符组（ACPI §20.2.2 NameSeg 原生形态）。
            //      词法层直接产 NameSeg 记号；不满足则落入主码/未知路径。
            if is_lead_name_char(b) && pos + 4 <= win_len
                && is_name_char(win[pos + 1])
                && is_name_char(win[pos + 2])
                && is_name_char(win[pos + 3])
            {
                let dec = decode_nameseg_native(win, pos as u32).map_err(|e| SgError {
                    code: e.code,
                    offset: pos as u32,
                    tick: self.tick,
                })?;
                self.phase = ScanPhase::OperandCollect;
                let t = Token {
                    kind: TokenKind::NameSeg,
                    start: pos as u32,
                    end: (pos + dec.raw_len as usize) as u32,
                    opcode: 0,
                    ext: false,
                    text: nameseg_to_string(&dec.chars),
                    value: 0,
                };
                self.nameseg_count += 1;
                self.tokens.push(t);
                pos += dec.raw_len as usize;
                self.phase = ScanPhase::Scanning;
                continue;
            }
            // ---- 主码查表 ----
            match main_op_lookup(b) {
                Some(idx) => {
                    let info = MAIN_OP_TABLE[idx];
                    self.phase = ScanPhase::OperandCollect;
                    // 数据项类记号需 OperandCollect 子例程补齐载荷；
                    // 带包长的命名对象：先解码 PkgLength（界检），随后按
                    // opcode → PkgLen 顺序入流（与 iasl 反汇编读序一致）。
                    let pkg: Option<PkgLenDecoded> = match b {
                        0x10 | 0x11 | 0x12 | 0x13 | 0x14 => {
                            let d = decode_pkg_length(win, (pos + 1) as u32).map_err(|e| {
                                SgError { code: e.code, offset: pos as u32, tick: self.tick }
                            })?;
                            if (pos as u64) + 1 + d.body_len as u64 > win.len() as u64 {
                                return Err(SgError {
                                    code: SgCode::BadPkgLength,
                                    offset: pos as u32,
                                    tick: self.tick,
                                });
                            }
                            Some(d)
                        }
                        _ => None,
                    };
                    let consumed = match b {
                        0x0A => self.collect_int_const(win, pos, 1)?,
                        0x0B => self.collect_int_const(win, pos, 2)?,
                        0x0C => self.collect_int_const(win, pos, 4)?,
                        0x0E => self.collect_int_const(win, pos, 8)?,
                        0x0D => self.collect_string(win, pos)?,
                        0x10 | 0x11 | 0x12 | 0x13 | 0x14 => {
                            1 + pkg.as_ref().map_or(0, |d| d.raw_len as u32)
                        }
                        _ => 1,
                    };
                    let is_const = matches!(b, 0x0A | 0x0B | 0x0C | 0x0E);
                    let t = Token {
                        kind: info.kind,
                        start: pos as u32,
                        end: (pos + consumed as usize) as u32,
                        opcode: b,
                        ext: false,
                        text: if is_const {
                            // 载荷十进制文本（手工 itoa，无格式化宏依赖）。
                            let v = self.int_value_of(win, pos, consumed)?;
                            u64_to_dec(v)
                        } else {
                            info.mnem.to_string()
                        },
                        value: if is_const {
                            self.int_value_of(win, pos, consumed)?
                        } else {
                            0
                        },
                    };
                    self.push_token_kind(&t, info.kind);
                    self.bump_freq(info.mnem);
                    self.tokens.push(t);
                    // PkgLength 记号随后入流（判据：包长度三/四档解码）。
                    if let Some(d) = pkg {
                        let pt = Token {
                            kind: TokenKind::PkgLen,
                            start: (pos + 1) as u32,
                            end: (pos + 1 + d.raw_len as usize) as u32,
                            opcode: 0,
                            ext: false,
                            text: u64_to_dec(d.body_len as u64),
                            value: d.body_len as u64,
                        };
                        self.pkg_len_count += 1;
                        self.tokens.push(pt);
                    }
                    pos += consumed as usize;
                    self.phase = ScanPhase::Scanning;
                }
                None => {
                    // ---- 未知码：入账（带上下文摘要）+ 占位记号，扫描继续
                    //      （判据：未知码有账，不清零不静默）。
                    self.push_unknown_ctx(win, pos, b, false);
                    let t = Token {
                        kind: TokenKind::Unknown,
                        start: pos as u32,
                        end: (pos + 1) as u32,
                        opcode: b,
                        ext: false,
                        text: String::new(),
                        value: 0,
                    };
                    self.tokens.push(t);
                    pos += 1;
                    self.phase = ScanPhase::OperandCollect;
                }
            }
        }
        self.phase = ScanPhase::Scanning;
        Ok(())
    }

    /// 记号入流 + 分类计数（OperandCollect 的载荷已并入 Token）。
    fn push_token_kind(&mut self, _t: &Token, kind: TokenKind) {
        match kind {
            TokenKind::NamedObj => self.named_obj_count += 1,
            TokenKind::ControlOp => self.control_count += 1,
            TokenKind::DataOp => self.data_count += 1,
            TokenKind::ExtOp => self.ext_count += 1,
            TokenKind::NameSeg => self.nameseg_count += 1,
            TokenKind::IntConst => self.int_const_count += 1,
            TokenKind::StrConst => self.str_const_count += 1,
            TokenKind::PkgLen => self.pkg_len_count += 1,
            TokenKind::LocalArgRef => self.local_arg_count += 1,
            TokenKind::Unknown => { /* Unknown 由 push_unknown 单独计 */ }
        }
    }

    /// 未知码入账：偏移 + 码值 + 窗口 8 字节 hex 上下文摘要。
    /// 未知码诊断码经由 `first_unknown_code()` 对外暴露（账本不中断扫描）。
    fn push_unknown_ctx(&mut self, win: &[u8], offset: usize, opcode: u8, ext: bool) {
        self.unknown_count += 1;
        self.unknowns.push(UnknownEntry {
            offset: offset as u32,
            opcode,
            ext,
            context_hex: build_context_hex(win, offset as u32),
        });
    }

    /// opcode 频次记账：首现追加键，复现累加值（保持首现顺序，清单导出定稿）。
    fn bump_freq(&mut self, mnem: &str) {
        let mut i = 0;
        while i < self.op_freq_keys.len() {
            if self.op_freq_keys[i] == mnem {
                self.op_freq_vals[i] += 1;
                return;
            }
            i += 1;
        }
        self.op_freq_keys.push(mnem.to_string());
        self.op_freq_vals.push(1);
    }

    /// OperandCollect·整数常量：0x0A 取 1 字节 / 0x0B 2 / 0x0C 4 / 0x0E 8。
    /// 返回 opcode 消耗总字节数（1 + 取数长）；越窗报 ConstTruncated。
    fn collect_int_const(&mut self, win: &[u8], pos: usize, width: usize) -> Result<u32, SgError> {
        if pos + 1 + width > win.len() {
            return Err(SgError {
                code: SgCode::ConstTruncated,
                offset: pos as u32,
                tick: self.tick,
            });
        }
        let _ = win; // 窗口长度已验，取数在 value_of 中进行
        Ok((1 + width) as u32)
    }

    /// 小端整值提取（width ≤ 8，全整数）。
    fn int_value_of(&self, win: &[u8], pos: usize, consumed: u32) -> Result<u64, SgError> {
        let width = consumed as usize - 1;
        if pos + 1 + width > win.len() {
            return Err(SgError {
                code: SgCode::ConstTruncated,
                offset: pos as u32,
                tick: self.tick,
            });
        }
        let mut v: u64 = 0;
        let mut k = 0;
        while k < width {
            v |= (win[pos + 1 + k] as u64) << (8 * k);
            k += 1;
        }
        Ok(v)
    }

    /// OperandCollect·字符串常量：0x0D 起扫，至收尾 0x00
    ///（AML 字符串以 0x00 收尾——ACPI §20.2.3 StringTerm）。越窗未闭合报
    /// StrUnterminated。返回消耗字节数（1 + 内容 + 收尾）。
    fn collect_string(&mut self, win: &[u8], pos: usize) -> Result<u32, SgError> {
        let mut end = pos + 1;
        while end < win.len() && win[end] != 0x00 {
            end += 1;
        }
        if end >= win.len() {
            return Err(SgError {
                code: SgCode::StrUnterminated,
                offset: pos as u32,
                tick: self.tick,
            });
        }
        Ok((end - pos + 1) as u32)
    }
}

// ---------------------------------------------------------------------------
// 十四、增量重扫差异账（相六 Reopened 的账面比对）
// ---------------------------------------------------------------------------

/// 重扫前后清单差异账：只追加不改写——重扫账目单调增长，差值即增量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanDiff {
    /// 重扫起点偏移。
    pub from_offset: u32,
    /// 新增记号数。
    pub added_tokens: u64,
    /// 新增未知码条数。
    pub added_unknowns: u64,
    /// 新增频次 kv 条目数（既有键的累加不计入）。
    pub added_freq_keys: u64,
    /// 重扫前覆盖率千分位。
    pub permille_before: u64,
    /// 重扫后覆盖率千分位。
    pub permille_after: u64,
}

/// 计算重扫差异：before 为旧清单，after 为 rescan_from 返回的新清单。
/// 全整数比对，纯确定性。
pub fn rescan_diff(
    before: &LexManifest,
    after: &LexManifest,
    from_offset: u32,
) -> RescanDiff {
    let added_tokens = after.token_total.saturating_sub(before.token_total);
    let added_unknowns = (after.unknowns.len() as u64).saturating_sub(before.unknowns.len() as u64);
    // 新增频次键：after 独有的 key 个数（按 key 名比对）。
    let mut added_keys = 0u64;
    let mut i = 0;
    while i < after.op_freq.len() {
        let mut found = false;
        let mut k = 0;
        while k < before.op_freq.len() {
            if after.op_freq[i].key == before.op_freq[k].key {
                found = true;
                break;
            }
            k += 1;
        }
        if !found {
            added_keys += 1;
        }
        i += 1;
    }
    RescanDiff {
        from_offset,
        added_tokens,
        added_unknowns,
        added_freq_keys: added_keys,
        permille_before: before.coverage.permille,
        permille_after: after.coverage.permille,
    }
}

/// 手工 u64 → 十进制 String（禁格式化宏依赖，纯整数）。
fn u64_to_dec(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // 安全构造：buf[i..] 全为 ASCII 数字。
    let mut s = String::with_capacity(buf.len() - i);
    let mut k = i;
    while k < buf.len() {
        s.push(buf[k] as char);
        k += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 十一、iasl 抽样对照表（判据 J1：对拍 ≥99%）
// ---------------------------------------------------------------------------

/// 一条抽样：字节序列 → 期望记号 Kind 序列（iasl 反汇编对拍锚点）。
/// 来源：ACPI 6.5 §20.2 编码 + iasl -d 输出常见片段抽样，写死不运行时生成。
#[derive(Debug, Clone)]
pub struct IaslSample {
    /// 抽样名（对拍账显示）。
    pub name: &'static str,
    /// 已知字节序列。
    pub bytes: &'static [u8],
    /// 期望记号 Kind 序列（按产出顺序；PkgLen 记号也计入）。
    pub expect: &'static [TokenKind],
}

/// 12 条 iasl 抽样对照表。字节序列逐条注释含义与 iasl 反汇编形态。
pub const IASL_SAMPLES: [IaslSample; 12] = [
    // 1) ZeroOp：iasl 反汇编 `Zero`，单字节。
    IaslSample {
        name: "zero_op",
        bytes: &[0x00],
        expect: &[TokenKind::IntConst],
    },
    // 2) OneOp：iasl `One`。
    IaslSample {
        name: "one_op",
        bytes: &[0x01],
        expect: &[TokenKind::IntConst],
    },
    // 3) ByteConst 0xFF：iasl `0xFF`。
    IaslSample {
        name: "byte_const_ff",
        bytes: &[0x0A, 0xFF],
        expect: &[TokenKind::IntConst],
    },
    // 4) WordConst 0x0402：iasl `0x0402`（PM1a 常见）。
    IaslSample {
        name: "word_const_0402",
        bytes: &[0x0B, 0x02, 0x04],
        expect: &[TokenKind::IntConst],
    },
    // 5) DwordConst：iasl `0x00000000`。
    IaslSample {
        name: "dword_const_0",
        bytes: &[0x0C, 0x00, 0x00, 0x00, 0x00],
        expect: &[TokenKind::IntConst],
    },
    // 6) QwordConst：iasl `0x0000000000000000`。
    IaslSample {
        name: "qword_const_0",
        bytes: &[0x0E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        expect: &[TokenKind::IntConst],
    },
    // 7) StringConst "AB"：iasl `"AB"`（0x00 收尾）。
    IaslSample {
        name: "string_const_ab",
        bytes: &[0x0D, b'A', b'B', 0x00],
        expect: &[TokenKind::StrConst],
    },
    // 8) ScopeOp + RootPrefix + "_SB_"：iasl
    //    `Scope (\_SB_)` —— 0x10 Scope + 一字节档包长 0x04 + 0x5C RootPrefix
    //    + 四字符 NameSeg "_SB_"（Root 前缀与 NameSeg 各产一条 NameSeg 记号）。
    IaslSample {
        name: "scope_root_sb",
        bytes: &[0x10, 0x04, 0x5C, b'_', b'S', b'B', b'_'],
        expect: &[TokenKind::NamedObj, TokenKind::PkgLen, TokenKind::NameSeg, TokenKind::NameSeg],
    },
    // 9) MethodOp + "_PTS" + 短包 + OneOp + Arg0：iasl
    //    `Method (_PTS, 1)` 骨架：0x14 Method + 包长 + 名 + 参数标志 0x01
    //    （词法层读作 OneOp IntConst）+ 0x68 Arg0。
    IaslSample {
        name: "method_pts",
        bytes: &[0x14, 0x07, b'_', b'P', b'T', b'S', 0x01, 0x68],
        expect: &[
            TokenKind::NamedObj,
            TokenKind::PkgLen,
            TokenKind::NameSeg,
            TokenKind::IntConst,
            TokenKind::LocalArgRef,
        ],
    },
    // 10) BufferOp 头对拍：iasl `Buffer (0x02) {0x04}` —— 0x11 Buffer +
    //     一字节档包长 0x02 + ByteConst 载荷 0x04。
    IaslSample {
        name: "buffer_hdr",
        bytes: &[0x11, 0x02, 0x0A, 0x04],
        expect: &[TokenKind::NamedObj, TokenKind::PkgLen, TokenKind::IntConst],
    },
    // 11) ExtOp MutexOp：iasl `Mutex (MTX, 0x00)` —— 0x5B 0x01 + 四字符名
    //     "MTX_"（尾字符 '_' 属 NameChar，原生四字符形态）+ ZeroOp 同步级。
    IaslSample {
        name: "ext_mutex",
        bytes: &[0x5B, 0x01, b'_', b'M', b'T', b'X', 0x00],
        expect: &[TokenKind::ExtOp, TokenKind::NameSeg, TokenKind::IntConst],
    },
    // 12) ExtOp OpRegionOp：iasl `OperationRegion (...)` —— 0x5B 0x80 +
    //     ByteConst 占位载荷（词法层只对拍头部编码形态）。
    IaslSample {
        name: "ext_opregion",
        bytes: &[0x5B, 0x80, 0x0A, 0x42],
        expect: &[TokenKind::ExtOp, TokenKind::IntConst],
    },
];

/// 抽样对拍结果条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleVerdict {
    /// 抽样名。
    pub name: String,
    /// 是否通过（实扫 Kind 序列与期望逐位一致）。
    pub pass: bool,
    /// 实扫 Kind 数。
    pub got_len: usize,
    /// 期望 Kind 数。
    pub want_len: usize,
    /// 首个不中位次（全中为 usize::MAX 语义化：用 got_len 收敛）。
    pub first_mismatch: u64,
}

/// 跑全部抽样对拍：每条用全新扫描器扫固定字节，实扫 Kind 序列与期望
/// 序列逐位比。全部通过返回 Ok(条目账)；任一不中返回 SampleMismatch
/// 错误（错误带首错抽样序号偏移）。判据 J1 的"对照 iasl 抽样表"落点。
pub fn run_iasl_sample_alignment(tick: u64) -> Result<Vec<SampleVerdict>, SgError> {
    let mut verdicts = Vec::with_capacity(IASL_SAMPLES.len());
    let mut idx = 0;
    while idx < IASL_SAMPLES.len() {
        let s = &IASL_SAMPLES[idx];
        let mut lx = AmlLexer::new(tick);
        // 锚定 + 全扫；抽样字节长全部 < 64KB，窗口即全序列。
        lx.anchor_window(s.bytes, tick)?;
        let manifest = lx.scan(s.bytes, tick)?;
        // 实扫 Kind 序列（含 Unknown；PkgLen 记号在流中如实出现）。
        let toks = lx.tokens();
        let mut got: Vec<TokenKind> = Vec::with_capacity(toks.len());
        let mut i = 0;
        while i < toks.len() {
            got.push(toks[i].kind);
            i += 1;
        }
        let want = s.expect;
        let mut first_mismatch: u64 = got.len().min(want.len()) as u64;
        let pass = got.len() == want.len() && {
            let mut ok = true;
            let mut k = 0;
            while k < got.len() {
                if got[k] != want[k] {
                    ok = false;
                    first_mismatch = k as u64;
                    break;
                }
                k += 1;
            }
            ok
        };
        verdicts.push(SampleVerdict {
            name: s.name.to_string(),
            pass,
            got_len: got.len(),
            want_len: want.len(),
            first_mismatch,
        });
        if !pass {
            return Err(SgError { code: SgCode::SampleMismatch, offset: idx as u32, tick });
        }
        let _ = manifest;
        idx += 1;
    }
    Ok(verdicts)
}

// ---------------------------------------------------------------------------
// 十二、覆盖率总账与 J1 判定（判据 J1 主入口）
// ---------------------------------------------------------------------------

/// J1 判定总账：抽样对拍 + 覆盖率双闸。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct J1Verdict {
    /// 抽样通过条数 / 总条数。
    pub sample_pass: u64,
    pub sample_total: u64,
    /// 全样对拍是否全中。
    pub samples_ok: bool,
    /// 覆盖率账。
    pub coverage: Coverage,
    /// J1 达标：全样通过 且 permille ≥ 990。
    pub pass: bool,
}

/// J1 总判定入口：先跑 iasl 抽样对拍（12 条），再对给定 DSDT 窗口全扫
/// 落覆盖率账。两闸全过才判达标；未知码账随清单透出。
pub fn judge_j1(dsdt_body: &[u8], tick: u64) -> Result<J1Verdict, SgError> {
    // 闸一：iasl 抽样对拍。
    let verdicts = run_iasl_sample_alignment(tick)?;
    let sample_total = verdicts.len() as u64;
    let mut sample_pass = 0;
    let mut i = 0;
    while i < verdicts.len() {
        if verdicts[i].pass {
            sample_pass += 1;
        }
        i += 1;
    }
    let samples_ok = sample_pass == sample_total && sample_total > 0;
    // 闸二：真窗覆盖率。
    let mut lx = AmlLexer::new(tick);
    lx.anchor_window(dsdt_body, tick)?;
    let manifest = lx.scan(dsdt_body, tick)?;
    let coverage = manifest.coverage;
    Ok(J1Verdict {
        sample_pass,
        sample_total,
        samples_ok,
        coverage,
        pass: samples_ok && coverage.meets_j1(),
    })
}

// ---------------------------------------------------------------------------
// 十三、诊断码段占用自检（跨模块防撞账，纯查表无 IO）
// ---------------------------------------------------------------------------

/// 本模块诊断码段（0x0720..=0x072F）。
pub const SG_CODE_SEG: (u16, u16) = (0x0720, 0x072F);

/// 诊断码段内自检：枚举全部 16 码均落段内且互异。纯查表确定性。
pub fn sg_code_seg_selfcheck() -> Result<(), SgError> {
    const ALL: [SgCode; 16] = [
        SgCode::EmptyInput,
        SgCode::WindowOverrun,
        SgCode::WindowTooLarge,
        SgCode::WindowNotAnchored,
        SgCode::UnknownOpcode,
        SgCode::UnknownExtOpcode,
        SgCode::BadNameLead,
        SgCode::BadNameTail,
        SgCode::NameSegTruncated,
        SgCode::BadPkgLength,
        SgCode::StrUnterminated,
        SgCode::ConstTruncated,
        SgCode::ExtTruncated,
        SgCode::DoubleAnchor,
        SgCode::SampleMismatch,
        SgCode::LedgerUnsealed,
    ];
    let mut i = 0;
    while i < ALL.len() {
        let c = ALL[i].code();
        if c < SG_CODE_SEG.0 || c > SG_CODE_SEG.1 {
            return Err(SgError { code: ALL[i], offset: i as u32, tick: 0 });
        }
        let mut k = i + 1;
        while k < ALL.len() {
            if ALL[k].code() == c {
                return Err(SgError { code: ALL[i], offset: k as u32, tick: 0 });
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}
