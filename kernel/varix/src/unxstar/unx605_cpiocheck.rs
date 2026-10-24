//! unx605_cpiocheck —— UNX-F0605 条目头校验和账（c_check 复核）。
//!
//! 判据原文摘录（docs/Varix/CoRun Varix STAR II · Unxreal 主册 · UNX-A1-B31 批）：
//!   | UNX-F0605 | 条目头校验和账（c_check 复核） | 200 | 已深化 | 校验和复核账齐（逐条目全核——c_check 与数据实测和 mod 65536 一致），失配检出注入红 |
//! 定位原文：批节标题「UNX-A1-B31 · initrd 只读桩（F601–F620 · 20 条）批」表内
//!   第 5 行（F0601–F0612 序）；批头防重注原文：「AI-01 承办｜域账累计：146270 +
//!   本批 4340 = 150610 / 240,000｜防重：initrd 归档只读账（识别/遍历/查找/红线）
//!   归本批独占；文件系统语义（写/挂载/缓存）属后续域（本批纯只读桩）；B19 内存
//!   账与 B23 表区登记为映射联动消费方｜R-A1-004 纪律：批小计 = 逐条求和唯一
//!   真值（本批逐条求和 4340）｜新增骨架批次（本会话立）」。
//!
//! 本模块落地（全手工定制真实子系统，零 crate:: 依赖，no_std + alloc）：
//!   1. `CheckVerify` 校验和复核：newc 头 c_check 字段两形态语义——0 形态
//!      （newc "070701" 档案约定：GNU cpio 对 newc 恒写 0，Linux 内核
//!      usr/gen_init_cpio 产档同例）跳过比对但记账；和值形态（"070702" newcrc
//!      族携真实和值；宽容口径下 070701 档亦可显式携值）逐条目与主口径实测和
//!      mod 65536 比对，失配即红案并定位条目序号/名字/头偏移/申报值/双口径实测值。
//!   2. `SumEngine` 求和引擎：16 位折叠和双口径——口径 A（FieldFold）13 个 hex
//!      字段解析值逐字段累加 wrap 65536（c_check 字段自身按惯例计 0：校验字段
//!      不得参与自身校验，BSD cpio(5) 历史口径以全空格计 check 位，本模块声明
//!      为 0 权重）；口径 B（AsciiBytes）110 头 ASCII 字节逐字节和 mod 65536
//!      （check 8 字节按 '0' 计）。双口径并记 + 口径差异归因（逐字段
//!      |ASCII 贡献 − 折和贡献| argmax 定位主导差异字段）。
//!   3. `IntegrityReport` 归档完整性面：逐条目复核账（条目名/c_check 形态/申报
//!      值/双口径实测和/结论）+ 整档通过率（千分率）+ 失配条目清单（红行渲染）
//!      + TRAILER 收档账注 + 尾后残余字节账注。
//!   4. 失配注入面：坏 c_check 值注入（检出 100% 自证 + 定位到条目序号 + 失配
//!      红账）、口径混用注入（check 携另一口径和 → CaliberCrossed 红分类）、
//!      遍历未过注入（魔数破坏 → 遍历拒收 → 校验拒收）。
//!   5. F0604 遍历对接：`TraverseFeed` 接口声明（镜像 unx604 `CpioIterator`/
//!      `static_walk` 输出形状：13 字段值组/名字/数据窗口/游标跨度/收档位）；
//!      遍历不过（feed.ok=false）→ `verify_feed` 拒收（0x1FD7）。
//!   6. 事件账（`CheckLedger`）+ `health()` + FNV-1a 32 指纹（报告指纹/账本
//!      指纹）+ 双跑确定性对拍（同档两次独立全走序指纹恒等）。
//!
//! 纪律：内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert
//!   除外）；可失败全走 Result<T, CpioCheckError>；错误枚举逐变体 detail() 中文
//!   可操作诊断；溢出显性 saturating/wrapping/checked；>64KB 结构体禁上栈（全部
//!   大件走 Vec 堆）；const fn 内零 slice::get/Vec 索引/Ord::min,max/derive ==
//!   /for 循环（while+索引替代）。诊断码段独占 **0x1FD0~0x1FDF**（先 grep 全仓
//!   确认 0x1FD 前缀零命中；0x1F50 段被本会话 F0587（unx0587_dupkey2.rs）占用、
//!   0x1F40~0x1F7F 被 unx2782_ubuddymerge.rs 头注声明覆盖、0x2000 命中均为普通
//!   地址非诊断码段，故顺位取 0x1FD0 段）。

use alloc::{format, string::String, vec, vec::Vec};

// ==========================================================================
// 版本与诊断码段
// ==========================================================================

/// 模块版本。
pub const V605C_VERSION: &str = "v605-cpiocheck-v1";

/// 诊断段基址（0x1FD0）。段 0x1FD0~0x1FDF 归本模块独占（grep 全仓确认 0x1FD
/// 前缀零命中——0x1F50 已被 unx0587_dupkey2.rs 占用，0x1F40~0x1F7F 被
/// unx2782_ubuddymerge.rs 声明覆盖，故顺位取本段）。
pub const V605C_SEG_BASE: u16 = 0x1FD0;

/// 诊断段末址（0x1FDF）。
pub const V605C_SEG_END: u16 = 0x1FDF;

/// 诊断码（16 码互斥闭集，覆盖本模块全部可诊断条件；正常事件不占诊断码，
/// 走 CheckEventKind 标签，异常事件携带 Option<u16> 指向本闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V605cCode {
    /// 0x1FD0 头短于 110 字节（newc 头被截断/空档）。
    HeadShort,
    /// 0x1FD1 魔数非 "070701"/"070702"（newc/newcrc 双族之外，含坏魔数注入）。
    MagicMismatch,
    /// 0x1FD2 整数字段含非十六进制字符（8 位 ASCII hex 域被破坏）。
    HexFieldBad,
    /// 0x1FD3 c_namesize = 0（newc 规定名区至少含 1 个 NUL）。
    NamesizeZero,
    /// 0x1FD4 名区末字节非 NUL（名字未按规格以 NUL 终止）。
    NameNulMissing,
    /// 0x1FD5 名区/数据区越出归档缓冲（越界注入或截断档）。
    DataOverrun,
    /// 0x1FD6 对齐垫字节非零（canonical newc 垫必须写 0）。
    PadNonZero,
    /// 0x1FD7 F0604 遍历未过，校验账拒收（feed.ok=false 或内走失败上抛）。
    TraversalReject,
    /// 0x1FD8 c_check > 0xFFFF（16 位折和域容纳不下的申报值，恒红）。
    CheckOutOfRange,
    /// 0x1FD9 c_check 与主口径实测和失配（含 newcrc 族携 0 值违约）。
    CheckMismatch,
    /// 0x1FDA c_check 命中另一口径实测和（口径混用注入——值对但口径错）。
    CaliberCrossed,
    /// 0x1FDB 归档未以 TRAILER!!! 终止（走查耗尽未见收档条目）。
    TrailerMissing,
    /// 0x1FDC 校验账失守（分账计数之和 ≠ 总条目数——复核面内部守卫）。
    LedgerMiss,
    /// 0x1FDD 双跑指纹失配（同档两次独立全走序指纹不恒等）。
    FingerprintMiss,
    /// 0x1FDE 注入面检出率失守（演练注入未被检出——检出率 < 100%）。
    InjectionUndetected,
    /// 0x1FDF 状态机守卫（封账后再入/走查条目数越上界等内部违例）。
    StateGuard,
}

impl V605cCode {
    /// 全量闭集（与段内 16 码一一对应，序即码升序）。
    pub const ALL: [V605cCode; 16] = [
        V605cCode::HeadShort,
        V605cCode::MagicMismatch,
        V605cCode::HexFieldBad,
        V605cCode::NamesizeZero,
        V605cCode::NameNulMissing,
        V605cCode::DataOverrun,
        V605cCode::PadNonZero,
        V605cCode::TraversalReject,
        V605cCode::CheckOutOfRange,
        V605cCode::CheckMismatch,
        V605cCode::CaliberCrossed,
        V605cCode::TrailerMissing,
        V605cCode::LedgerMiss,
        V605cCode::FingerprintMiss,
        V605cCode::InjectionUndetected,
        V605cCode::StateGuard,
    ];

    /// 码值（与声明序恒等，段内连续升序）。
    pub const fn code(self) -> u16 {
        match self {
            V605cCode::HeadShort => 0x1FD0,
            V605cCode::MagicMismatch => 0x1FD1,
            V605cCode::HexFieldBad => 0x1FD2,
            V605cCode::NamesizeZero => 0x1FD3,
            V605cCode::NameNulMissing => 0x1FD4,
            V605cCode::DataOverrun => 0x1FD5,
            V605cCode::PadNonZero => 0x1FD6,
            V605cCode::TraversalReject => 0x1FD7,
            V605cCode::CheckOutOfRange => 0x1FD8,
            V605cCode::CheckMismatch => 0x1FD9,
            V605cCode::CaliberCrossed => 0x1FDA,
            V605cCode::TrailerMissing => 0x1FDB,
            V605cCode::LedgerMiss => 0x1FDC,
            V605cCode::FingerprintMiss => 0x1FDD,
            V605cCode::InjectionUndetected => 0x1FDE,
            V605cCode::StateGuard => 0x1FDF,
        }
    }

    /// 码值反查（无码命中返回 None——诊断面闭环自查）。
    pub const fn from_code(value: u16) -> Option<V605cCode> {
        match value {
            0x1FD0 => Some(V605cCode::HeadShort),
            0x1FD1 => Some(V605cCode::MagicMismatch),
            0x1FD2 => Some(V605cCode::HexFieldBad),
            0x1FD3 => Some(V605cCode::NamesizeZero),
            0x1FD4 => Some(V605cCode::NameNulMissing),
            0x1FD5 => Some(V605cCode::DataOverrun),
            0x1FD6 => Some(V605cCode::PadNonZero),
            0x1FD7 => Some(V605cCode::TraversalReject),
            0x1FD8 => Some(V605cCode::CheckOutOfRange),
            0x1FD9 => Some(V605cCode::CheckMismatch),
            0x1FDA => Some(V605cCode::CaliberCrossed),
            0x1FDB => Some(V605cCode::TrailerMissing),
            0x1FDC => Some(V605cCode::LedgerMiss),
            0x1FDD => Some(V605cCode::FingerprintMiss),
            0x1FDE => Some(V605cCode::InjectionUndetected),
            0x1FDF => Some(V605cCode::StateGuard),
            _ => None,
        }
    }

    /// 中文诊断名（短）。
    pub const fn name(self) -> &'static str {
        match self {
            V605cCode::HeadShort => "头短于 110 字节",
            V605cCode::MagicMismatch => "魔数非 070701/070702",
            V605cCode::HexFieldBad => "整数字段含非十六进制字符",
            V605cCode::NamesizeZero => "namesize 为 0",
            V605cCode::NameNulMissing => "名区末字节非 NUL",
            V605cCode::DataOverrun => "名区/数据区越出归档缓冲",
            V605cCode::PadNonZero => "对齐垫字节非零",
            V605cCode::TraversalReject => "F0604 遍历未过校验拒收",
            V605cCode::CheckOutOfRange => "c_check 越出 16 位和域",
            V605cCode::CheckMismatch => "c_check 与实测和失配",
            V605cCode::CaliberCrossed => "c_check 命中另一口径",
            V605cCode::TrailerMissing => "归档未以 TRAILER 终止",
            V605cCode::LedgerMiss => "校验账失守",
            V605cCode::FingerprintMiss => "双跑指纹失配",
            V605cCode::InjectionUndetected => "注入面检出率失守",
            V605cCode::StateGuard => "状态机守卫",
        }
    }

    /// 中文可操作诊断（长——修复指引）。
    pub const fn detail(self) -> &'static str {
        match self {
            V605cCode::HeadShort => "归档缓冲不足 110 字节（或空档），无法容纳一个完整 newc 头：检查 UNX-F0601 基址/长度交接是否截短，或上一切割点游标推进是否少算对齐垫。",
            V605cCode::MagicMismatch => "条目头前 6 字节不在 newc/newcrc 双族魔数（\"070701\"/\"070702\"）闭集内：确认归档由 cpio -H newc（或 newcrc）生成；若首条目即坏查 F0603 魔数账，若中段坏查前条目 span 推进。",
            V605cCode::HexFieldBad => "13 个 8 位十六进制整数字段中出现非 [0-9a-fA-F] 字符：头部被覆写或生成器损坏；本模块复核账以字段解析为前置，先用 F0604 遍历账定位坏条目偏移再重展开归档。",
            V605cCode::NamesizeZero => "c_namesize = 0 违反 newc 规格（名区至少含终止 NUL 故 namesize ≥ 1）：生成器 bug 或字段错位；核对字段序 c_namesize 为第 12 字段（偏移 94）。",
            V605cCode::NameNulMissing => "名区最后一个字节不是 NUL：名字未按规格终止，条目边界不可信；拒收并核对生成器是否少写 1 字节 NUL（namesize 应为名长+1）。",
            V605cCode::DataOverrun => "名区或数据区终点越出归档缓冲（长度交接过短或 c_filesize 被注水）：核对 UNX-F0601 长度账与 c_filesize 字段；截断档需重新展开，不得带伤复核。",
            V605cCode::PadNonZero => "对齐垫字节非零（canonical newc 名区垫/数据垫必须写 0）：生成器在垫位写垃圾破坏逐字节往返一致；本模块走查按 F0604 同口径拒收，重生成归档。",
            V605cCode::TraversalReject => "F0604 遍历未过，校验账拒收（遍历不过则校验无从谈起）：先修遍历红（魔数/hex/越界/垫/NUL/TRAILER 各类），复核账只在遍历绿档上开展。",
            V605cCode::CheckOutOfRange => "c_check 申报值 > 0xFFFF：16 位折叠和域（mod 65536）容纳不下该值，恒为红案；核对生成器是否把 32 位原始和未折叠写入，或字段错位读到了相邻数据。",
            V605cCode::CheckMismatch => "c_check 申报值与主口径实测和（mod 65536）失配：头字段被篡改或 check 写入用了异口径；看红行的双口径实测值——若恰等于另一口径和则按口径混用归因（0x1FDA）。",
            V605cCode::CaliberCrossed => "c_check 值本身正确但取的是另一求和口径（口径混用）：确认产档器与本复核器主口径一致（FieldFold=字段值折和 / AsciiBytes=头 ASCII 字节和），统一后重产档。",
            V605cCode::TrailerMissing => "归档未以 \"TRAILER!!!\" 条目终止：尾部被截断或游标提前耗尽；核对归档总长与末条目 span（F0607 尾核域详账），补全后再入复核。",
            V605cCode::LedgerMiss => "复核账内部失守（零形态绿+匹配绿+各类红 ≠ 总条目数）：这是复核器自身缺陷信号而非档的问题；升级模块版本并保留现场档做回归。",
            V605cCode::FingerprintMiss => "双跑确定性失配（同档两次独立全走序 FNV 指纹不恒等）：复核器存在隐藏状态泄漏；禁用该复核器实例并用新实例复跑取证。",
            V605cCode::InjectionUndetected => "注入演练检出率 < 100%（有坏档未被红案捕获）：复核面存在漏检；逐 case 对账 DrillReport 定位漏检类别并先修后复产。",
            V605cCode::StateGuard => "状态机违例（封账后再入 / 走查条目数越 65536 上界 / 游标非单调）：检查是否存在并发复用同一复核器或恶意超长档循环注入。",
        }
    }
}

// 编译期账：段内 16 码互斥、连续、首尾对齐段界。
const _: () = assert!(V605cCode::ALL.len() == 16);
const _: () = assert!(V605cCode::ALL[0].code() == V605C_SEG_BASE);
const _: () = assert!(V605cCode::ALL[15].code() == V605C_SEG_END);

// ==========================================================================
// 统一错误类型
// ==========================================================================

/// 校验域统一错误（Copy 小件，逐错带条目序号与归档内偏移留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CpioCheckError {
    code: V605cCode,
    entry_index: u32,
    offset: u32,
}

impl CpioCheckError {
    /// 构造（entry_index 为出错时正在复核的条目序号，offset 为归档内字节偏移；
    /// usize 偏移超 u32 域时收口到 u32::MAX 留痕——不截断为 0 掩盖现场）。
    pub fn new(code: V605cCode, entry_index: u32, offset: usize) -> Self {
        let off32 = if offset > u32::MAX as usize {
            u32::MAX
        } else {
            offset as u32
        };
        CpioCheckError {
            code,
            entry_index,
            offset: off32,
        }
    }

    /// 诊断码。
    pub const fn code(&self) -> V605cCode {
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
// 手写十六进制编解码（零依赖——本模块自持，不引 unx604 符号）
// ==========================================================================

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
/// 长度非 8 或含非十六进制字符 → Err(HexFieldBad)。
pub fn parse_hex8(bytes: &[u8]) -> Result<u32, V605cCode> {
    if bytes.len() != FIELD_HEX_LEN {
        return Err(V605cCode::HexFieldBad);
    }
    let mut value: u32 = 0;
    let mut i: usize = 0;
    while i < FIELD_HEX_LEN {
        let digit = match hex_digit_value(bytes[i]) {
            Some(d) => d,
            None => return Err(V605cCode::HexFieldBad),
        };
        value = (value << 4) | (digit as u32);
        i += 1;
    }
    Ok(value)
}

/// 编码一个 u32 为 8 位 ASCII 小写十六进制（定长输出）。
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

/// 把一个整数字段原位写回缓冲（注入面/修复面共用）；越界返回 false 不写。
pub fn write_hex8_into(buf: &mut [u8], field_offset: usize, value: u32) -> bool {
    if field_offset > buf.len() || buf.len() - field_offset < FIELD_HEX_LEN {
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
// newc/newcrc 格式常量（来源逐条注释）
// ==========================================================================

/// newc 魔数 "070701"（GNU cpio copyin.c / Linux usr/gen_init_cpio 规格：
/// 6 字节 ASCII 魔数，check 字段恒写 0）。
pub const NEWC_MAGIC: [u8; 6] = *b"070701";

/// newcrc 魔数 "070702"（GNU cpio 规格：CRC 格式族，check 字段携校验和值）。
pub const NEWCRC_MAGIC: [u8; 6] = *b"070702";

/// 魔数长度。
pub const MAGIC_LEN: usize = 6;

/// 单个整数字段十六进制长度（newc 规定每整数字段 8 位 ASCII hex）。
pub const FIELD_HEX_LEN: usize = 8;

/// newc 条目头总长 = 6 魔数 + 13 × 8 hex = 110 字节（GNU cpio newc 规格）。
pub const HEADER_LEN: usize = MAGIC_LEN + 13 * FIELD_HEX_LEN;

/// 归档内对齐界：名区与数据区均按 4 字节边界收口（newc 规格 + Linux 内核
/// initramfs unpack 实现同口径）。
pub const ARCHIVE_ALIGNMENT: usize = 4;

/// 名区终止字节（NUL）。
pub const NAME_TERMINATOR: u8 = 0;

/// TRAILER 终止符条目名（GNU cpio 规格：以该名条目收档）。
pub const TRAILER_NAME: &str = "TRAILER!!!";

/// 13 个整数字段名序（newc 规格字段序，偏移 = 6 + 8×序）。
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

/// 13 个整数字段头内偏移（6 + 8×序，逐值写死可直查）。
pub const FIELD_OFFSETS: [usize; 13] = [
    6, 14, 22, 30, 38, 46, 54, 62, 70, 78, 86, 94, 102,
];

/// c_filesize 字段序（第 7 字段——数据区长度账）。
pub const FIELD_FILESIZE: usize = 6;

/// c_namesize 字段序（第 12 字段——名区长度账）。
pub const FIELD_NAMESIZE: usize = 11;

/// c_check 字段序（第 13 字段——本模块复核主体）。
pub const FIELD_CHECK: usize = 12;

/// c_check 字段头内偏移（6 + 8×12 = 102）。
pub const FIELD_CHECK_OFFSET: usize = 6 + 8 * FIELD_CHECK;

/// 16 位折叠和模数（cpio check 域 16 位和约定：字段和 mod 65536）。
pub const CHECK_MODULUS: u32 = 65536;

/// 走查条目数上界（防恶意循环档把复核器拖死：65536 条 ≈ 最小档 720KB ×
/// 65536 已远超 initramfs 现实规模；守卫值自定）。
pub const MAX_WALK_ENTRIES: usize = 65536;

/// 路径名长度预算上界（VFS PATH_MAX 折算 4096；namesize 含 NUL 不得越界，
/// 产档面守卫用；走查面不以此拒收——长度账归 F0606 域）。
pub const MAX_NAME_BYTES: usize = 4096;

/// 产档面默认 mtime（2023-11-14 22:13:20 UTC ≈ 1_700_000_000，测试档定值
/// 保证双跑指纹确定）。
pub const BUILD_MTIME: u32 = 1_700_000_000;

// 编译期账：头长守恒式 + 字段偏移表首尾对齐。
const _: () = assert!(HEADER_LEN == 6 + 13 * 8);
const _: () = assert!(HEADER_LEN == 110);
const _: () = assert!(FIELD_OFFSETS[0] == 6);
const _: () = assert!(FIELD_OFFSETS[12] == 102);
const _: () = assert!(FIELD_CHECK_OFFSET == 102);

// ==========================================================================
// FNV-1a 32 指纹（自持实现——报告指纹/账本指纹/双跑对拍共用）
// ==========================================================================

/// FNV-1a 32 位偏移基（FNV-1a 规格常量）。
pub const FNV_OFFSET_BASIS: u32 = 0x811C_9DC5;

/// FNV-1a 32 位素数（FNV-1a 规格常量）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 流式指纹器（复核报告指纹与事件账指纹的确定性底座）。
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a32 {
    state: u32,
}

impl Fnv1a32 {
    /// 新指纹器（初态 = FNV 偏移基）。
    pub const fn new() -> Fnv1a32 {
        Fnv1a32 {
            state: FNV_OFFSET_BASIS,
        }
    }

    /// 推入单字节。
    pub fn push(&mut self, byte: u8) {
        self.state ^= byte as u32;
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }

    /// 推入字节切片。
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let mut i: usize = 0;
        while i < bytes.len() {
            self.push(bytes[i]);
            i += 1;
        }
    }

    /// 推入 u32（小端序——跨运行字节序确定）。
    pub fn push_u32le(&mut self, value: u32) {
        self.push((value & 0xFF) as u8);
        self.push(((value >> 8) & 0xFF) as u8);
        self.push(((value >> 16) & 0xFF) as u8);
        self.push(((value >> 24) & 0xFF) as u8);
    }

    /// 推入 u16（小端序）。
    pub fn push_u16le(&mut self, value: u16) {
        self.push((value & 0xFF) as u8);
        self.push(((value >> 8) & 0xFF) as u8);
    }

    /// 推入 u8 单值（语义面用——与 push 同构但显名）。
    pub fn push_u8(&mut self, value: u8) {
        self.push(value);
    }

    /// 终态指纹。
    pub const fn finish(&self) -> u32 {
        self.state
    }

    /// 一次性切片指纹（便捷面）。
    pub fn digest(bytes: &[u8]) -> u32 {
        let mut h = Fnv1a32::new();
        h.push_bytes(bytes);
        h.finish()
    }
}

// ==========================================================================
// 魔数族（newc / newcrc）
// ==========================================================================

/// cpio 格式族判别（本模块受理闭集：newc 与 newcrc 两族；其余拒收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MagicFamily {
    /// newc（魔数 "070701"，check 恒 0 档案约定族）。
    Newc,
    /// newcrc（魔数 "070702"，check 携和值族）。
    Newcrc,
}

impl MagicFamily {
    /// 从 6 字节魔数判别（const fn 手写逐字节比对——禁 for 循环用 while）。
    pub const fn from_magic(bytes: &[u8]) -> Option<MagicFamily> {
        if bytes.len() != MAGIC_LEN {
            return None;
        }
        // newc "070701"
        let mut is_newc: bool = true;
        let mut i: usize = 0;
        while i < MAGIC_LEN {
            if bytes[i] != NEWC_MAGIC[i] {
                is_newc = false;
            }
            i += 1;
        }
        if is_newc {
            return Some(MagicFamily::Newc);
        }
        // newcrc "070702"
        let mut is_newcrc: bool = true;
        let mut j: usize = 0;
        while j < MAGIC_LEN {
            if bytes[j] != NEWCRC_MAGIC[j] {
                is_newcrc = false;
            }
            j += 1;
        }
        if is_newcrc {
            return Some(MagicFamily::Newcrc);
        }
        None
    }

    /// 族中文名。
    pub const fn name(self) -> &'static str {
        match self {
            MagicFamily::Newc => "newc(070701)",
            MagicFamily::Newcrc => "newcrc(070702)",
        }
    }

    /// 族判别位（指纹用：0=newc，1=newcrc）。
    pub const fn id(self) -> u8 {
        match self {
            MagicFamily::Newc => 0,
            MagicFamily::Newcrc => 1,
        }
    }

    /// 本族 check 字段语义约定（true = 规格要求携和值）。
    pub const fn requires_value_check(self) -> bool {
        match self {
            MagicFamily::Newc => false,
            MagicFamily::Newcrc => true,
        }
    }
}

// ==========================================================================
// 对齐垫（4 字节界）
// ==========================================================================

/// 收口到 4 字节界所需的垫字节数（0..=3；const fn 纯算术）。
pub const fn pad4(total: usize) -> usize {
    (ARCHIVE_ALIGNMENT - (total % ARCHIVE_ALIGNMENT)) % ARCHIVE_ALIGNMENT
}

/// 收口到 4 字节界后的偏移（饱和加防溢出）。
pub const fn align4(total: usize) -> usize {
    total.saturating_add(pad4(total))
}

// 编译期账：垫界性质。
const _: () = assert!(pad4(0) == 0);
const _: () = assert!(pad4(1) == 3);
const _: () = assert!(pad4(4) == 0);
const _: () = assert!(align4(110) == 112);

// ==========================================================================
// 110 头解析（本模块自持——复核面以字段解析为前置）
// ==========================================================================

/// 解析归档 at 偏移处的 110 字节 newc 头为 13 个 u32 字段值。
/// 短头 → HeadShort；魔数坏 → MagicMismatch；hex 坏 → HexFieldBad。
pub fn parse_head_at(archive: &[u8], at: usize) -> Result<[u32; 13], CpioCheckError> {
    if at > archive.len() || archive.len() - at < HEADER_LEN {
        return Err(CpioCheckError::new(V605cCode::HeadShort, 0, at));
    }
    let head = &archive[at..at + HEADER_LEN];
    match MagicFamily::from_magic(&head[..MAGIC_LEN]) {
        Some(_) => {}
        None => return Err(CpioCheckError::new(V605cCode::MagicMismatch, 0, at)),
    }
    let mut fields: [u32; 13] = [0; 13];
    let mut i: usize = 0;
    while i < 13 {
        let off = FIELD_OFFSETS[i];
        match parse_hex8(&head[off..off + FIELD_HEX_LEN]) {
            Ok(v) => fields[i] = v,
            Err(_) => {
                return Err(CpioCheckError::new(
                    V605cCode::HexFieldBad,
                    0,
                    at + off,
                ))
            }
        }
        i += 1;
    }
    Ok(fields)
}

/// 取 at 偏移处 110 字节头窗（已保证界内时零拷贝借出；界外返回 None）。
pub fn head_window(archive: &[u8], at: usize) -> Option<&[u8]> {
    if at > archive.len() || archive.len() - at < HEADER_LEN {
        return None;
    }
    Some(&archive[at..at + HEADER_LEN])
}

// ==========================================================================
// 轻量走查（light_walk）——复核面内走器：与 F0604 遍历器同口径的结构防线
// ==========================================================================

/// 走查产出的单条目账（条目序号/头偏移/字段组/名字/数据窗口/跨度终点）。
#[derive(Clone, Debug)]
pub struct WalkEntry {
    /// 条目序号（0 起，含 TRAILER）。
    pub index: u32,
    /// 头 110 字节在归档内的偏移。
    pub head_off: usize,
    /// 13 字段解析值组。
    pub fields: [u32; 13],
    /// 条目名字节（不含终止 NUL；TRAILER 条目为 "TRAILER!!!"）。
    pub name: String,
    /// 名区（含 NUL）在归档内的偏移。
    pub name_off: usize,
    /// 数据区在归档内的偏移（已按 4 界对齐）。
    pub data_off: usize,
    /// 数据区长度（c_filesize 原值）。
    pub data_len: u32,
    /// 本条目跨度终点（头+名区+垫+数据+垫；下一头起点）。
    pub span_end: usize,
    /// 是否 TRAILER 收档条目。
    pub is_trailer: bool,
    /// 格式族（newc/newcrc）。
    pub family: MagicFamily,
}

impl WalkEntry {
    /// 名字渲染（诊断面）。
    pub fn name_display(&self) -> &str {
        &self.name
    }
}

/// 走查产出（条目清单 + 收档位 + 尾后残余字节）。
pub struct WalkOutcome {
    /// 全部条目（含 TRAILER 条目本身，序连续）。
    pub entries: Vec<WalkEntry>,
    /// 是否见到 TRAILER!!! 收档条目。
    pub trailer_seen: bool,
    /// TRAILER 跨度终点之后的残余字节数（残余 ≠ 0 合法但记账——详账归 F0607）。
    pub residual_bytes: usize,
    /// 归档总长。
    pub archive_len: usize,
}

/// 复核面前置走查：结构防线全开（头长/魔数/hex/namesize/NUL/越界/垫/条目上界/
/// TRAILER 收档），任何失守即 Err——「遍历不过则校验拒收」的内走实现。
pub fn light_walk(archive: &[u8]) -> Result<WalkOutcome, CpioCheckError> {
    let len = archive.len();
    if len == 0 {
        return Err(CpioCheckError::new(V605cCode::HeadShort, 0, 0));
    }
    let mut entries: Vec<WalkEntry> = Vec::new();
    let mut cursor: usize = 0;
    let mut trailer_seen: bool = false;
    let mut residual: usize = 0;
    while cursor < len {
        let index = entries.len() as u32;
        // 边界防线一：残余不足一个完整头。
        if len - cursor < HEADER_LEN {
            return Err(CpioCheckError::new(V605cCode::HeadShort, index, cursor));
        }
        // 边界防线二：条目数上界（防循环档拖死复核器）。
        if entries.len() >= MAX_WALK_ENTRIES {
            return Err(CpioCheckError::new(V605cCode::StateGuard, index, cursor));
        }
        // 头解析（魔数 + 13 字段 hex 三道防线内嵌）。
        let fields = parse_head_at(archive, cursor)?;
        let family = match MagicFamily::from_magic(&archive[cursor..cursor + MAGIC_LEN]) {
            Some(f) => f,
            None => {
                return Err(CpioCheckError::new(V605cCode::MagicMismatch, index, cursor));
            }
        };
        // 边界防线三：namesize = 0 拒收（newc 名区至少含 1 个 NUL）。
        let namesize = fields[FIELD_NAMESIZE] as usize;
        if namesize == 0 {
            return Err(CpioCheckError::new(
                V605cCode::NamesizeZero,
                index,
                cursor + FIELD_OFFSETS[FIELD_NAMESIZE],
            ));
        }
        // 边界防线四：名区完整落界。
        let name_off = cursor + HEADER_LEN;
        let name_end = match name_off.checked_add(namesize) {
            Some(e) if e <= len => e,
            _ => {
                return Err(CpioCheckError::new(
                    V605cCode::DataOverrun,
                    index,
                    name_off,
                ));
            }
        };
        // 边界防线五：名区末字节 NUL。
        if archive[name_end - 1] != NAME_TERMINATOR {
            return Err(CpioCheckError::new(
                V605cCode::NameNulMissing,
                index,
                name_end - 1,
            ));
        }
        let name_bytes: &[u8] = &archive[name_off..name_end - 1];
        // 名区垫：4 界收口且垫字节必须为 0（档末恰好截在名区终点时容忍缺垫）。
        let data_off_raw = align4(name_end);
        let mut data_off: usize = data_off_raw;
        if data_off_raw > len {
            if name_end == len {
                data_off = len; // 档末无垫容忍（事件面另有账注）
            } else {
                return Err(CpioCheckError::new(
                    V605cCode::DataOverrun,
                    index,
                    name_end,
                ));
            }
        } else {
            let mut p: usize = name_end;
            while p < data_off {
                if archive[p] != 0 {
                    return Err(CpioCheckError::new(V605cCode::PadNonZero, index, p));
                }
                p += 1;
            }
        }
        // 数据区：c_filesize 完整落界。
        let data_len = fields[FIELD_FILESIZE];
        let data_end = match data_off.checked_add(data_len as usize) {
            Some(e) if e <= len => e,
            _ => {
                return Err(CpioCheckError::new(
                    V605cCode::DataOverrun,
                    index,
                    data_off,
                ));
            }
        };
        // 数据垫：4 界收口且垫字节必须为 0（档末恰好截在数据终点时容忍缺垫）。
        let span_raw = align4(data_end);
        let mut span_end: usize = span_raw;
        if span_raw > len {
            if data_end == len {
                span_end = len; // 档末无垫容忍
            } else {
                return Err(CpioCheckError::new(
                    V605cCode::DataOverrun,
                    index,
                    data_end,
                ));
            }
        } else {
            let mut q: usize = data_end;
            while q < span_end {
                if archive[q] != 0 {
                    return Err(CpioCheckError::new(V605cCode::PadNonZero, index, q));
                }
                q += 1;
            }
        }
        let is_trailer: bool = name_bytes == TRAILER_NAME.as_bytes();
        entries.push(WalkEntry {
            index,
            head_off: cursor,
            fields,
            name: String::from_utf8_lossy(name_bytes).into_owned(),
            name_off,
            data_off,
            data_len,
            span_end,
            is_trailer,
            family,
        });
        if is_trailer {
            trailer_seen = true;
            residual = len - span_end;
            break;
        }
        cursor = span_end;
    }
    if !trailer_seen {
        // 游标耗尽未见 TRAILER：收档失守（残余 0 也红——走查语义要求显式收档）。
        return Err(CpioCheckError::new(
            V605cCode::TrailerMissing,
            entries.len() as u32,
            cursor,
        ));
    }
    Ok(WalkOutcome {
        entries,
        trailer_seen,
        residual_bytes: residual,
        archive_len: len,
    })
}

// ==========================================================================
// SumEngine 求和引擎——16 位折叠和双口径 + 口径差异归因
// ==========================================================================

/// 求和口径（check 复核的「尺」：同一 110 头在两把尺下各得一个 16 位和）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SumCaliber {
    /// 口径 A：13 个 hex 字段解析值逐字段累加 mod 65536（c_check 自身计 0
    /// 权重——校验字段不得参与自身校验；BSD cpio(5) 历史口径以全空格计 check
    /// 位，本模块声明为 0 权重，与字段值语义一致）。
    FieldFold,
    /// 口径 B：110 头 ASCII 字节逐字节和 mod 65536（check 8 字节按 '0' 计——
    /// 与 newc 生成器写 0 值的 ASCII 形态恒一致，故该口径对 check 值不敏感）。
    AsciiBytes,
}

impl SumCaliber {
    /// 口径中文名。
    pub const fn name(self) -> &'static str {
        match self {
            SumCaliber::FieldFold => "字段值折和(13 字段,check 计 0)",
            SumCaliber::AsciiBytes => "头 ASCII 字节和(check 位按 '0' 计)",
        }
    }

    /// 口径判别位（指纹用：0=A，1=B）。
    pub const fn id(self) -> u8 {
        match self {
            SumCaliber::FieldFold => 0,
            SumCaliber::AsciiBytes => 1,
        }
    }

    /// 另一口径（对拍/口径混用归因用）。
    pub const fn other(self) -> SumCaliber {
        match self {
            SumCaliber::FieldFold => SumCaliber::AsciiBytes,
            SumCaliber::AsciiBytes => SumCaliber::FieldFold,
        }
    }
}

/// 单字段对 wrap 累加口径（口径 A）的 16 位贡献（= v mod 65536，即低 16 位）。
/// 数学账：wrap 累加 Σ v_i mod 65536 下字段 i 的贡献即 v_i mod 65536——v_i 的
/// 高 16 位部分恒为 65536 的整数倍，对 mod 65536 的贡献为 0（注意：hi+lo 型
/// 「一补码折叠」是 mod 65535 语义的进位回卷，与本题 mod 65536 口径不同，不采用）。
pub const fn fold_u32_to_u16(value: u32) -> u16 {
    (value & 0xFFFF) as u16
}

/// 口径 A 全式：13 字段解析值累加 mod 65536（c_check 字段序 12 显式计 0——
/// 自指排除；const fn 用 while+索引，禁 for/Ord::min）。
pub const fn fold_field_sum_16(fields: &[u32; 13]) -> u16 {
    let mut acc: u32 = 0;
    let mut i: usize = 0;
    while i < 13 {
        if i != FIELD_CHECK {
            acc = acc.wrapping_add(fields[i]);
        }
        i += 1;
    }
    (acc & 0xFFFF) as u16
}

/// 口径 B 全式：110 头 ASCII 字节和 mod 65536（check 8 字节按 '0' 计）。
/// 头长非 110 → Err(HeadShort)。
pub fn ascii_head_sum_16(head110: &[u8]) -> Result<u16, CpioCheckError> {
    if head110.len() != HEADER_LEN {
        return Err(CpioCheckError::new(V605cCode::HeadShort, 0, 0));
    }
    let mut acc: u32 = 0;
    let mut i: usize = 0;
    while i < HEADER_LEN {
        let byte: u8 = if i >= FIELD_CHECK_OFFSET && i < FIELD_CHECK_OFFSET + FIELD_HEX_LEN {
            b'0' // check 位按 '0' 计（口径声明：对 check 值不敏感）
        } else {
            head110[i]
        };
        acc = acc.wrapping_add(byte as u32);
        i += 1;
    }
    Ok((acc & 0xFFFF) as u16)
}

/// 单字段 8 个 ASCII hex 字节和（该字段对口径 B 的贡献；mod 65536 截断）。
pub fn ascii_field_sum_8(field_ascii: &[u8]) -> u16 {
    let mut acc: u32 = 0;
    let mut i: usize = 0;
    while i < field_ascii.len() {
        acc = acc.wrapping_add(field_ascii[i] as u32);
        i += 1;
    }
    (acc & 0xFFFF) as u16
}

/// 双口径读数（一次复核的完整「测量面」：双总+逐字段分账+差异归因）。
#[derive(Clone, Copy, Debug)]
pub struct SumReading {
    /// 口径 A 总和（字段值折和，check 计 0）。
    pub fold_total: u16,
    /// 口径 B 总和（ASCII 字节和，check 位按 '0' 计）。
    pub ascii_total: u16,
    /// 逐字段口径 A 贡献（fold 后的 16 位值；check 槽恒 0）。
    pub fold_parts: [u16; 13],
    /// 逐字段口径 B 贡献（8 ASCII 字节和；check 槽为 8×'0'=0x180）。
    pub ascii_parts: [u16; 13],
    /// 双口径总差（ascii_total − fold_total，带符号；归因面顶账值）。
    pub delta_signed: i32,
    /// 主导差异字段序（|ascii_parts[i] − fold_parts[i]| argmax，首个最大者）。
    pub dominant_field: usize,
    /// 主导字段差异幅度（|ascii_parts[dom] − fold_parts[dom]|）。
    pub dominant_gap: u32,
}

impl SumReading {
    /// 两口径是否一致（完全同值——极少见，名值字段小时可能重合）。
    pub fn calibers_agree(&self) -> bool {
        self.fold_total == self.ascii_total
    }

    /// 主导差异字段名（归因渲染面）。
    pub fn dominant_field_name(&self) -> &'static str {
        if self.dominant_field < 13 {
            FIELD_NAMES[self.dominant_field]
        } else {
            "?"
        }
    }

    /// 读数渲染（诊断面一行账）。
    pub fn render(&self) -> String {
        format!(
            "fold=0x{:04X} ascii=0x{:04X} Δ={} 主导差异字段={}(幅 {})",
            self.fold_total,
            self.ascii_total,
            self.delta_signed,
            self.dominant_field_name(),
            self.dominant_gap
        )
    }
}

/// 求和引擎（复核器的「尺仓」：持主口径，产双口径读数与归因）。
#[derive(Clone, Copy, Debug)]
pub struct SumEngine {
    primary: SumCaliber,
}

impl SumEngine {
    /// 默认引擎（主口径 A：FieldFold——判据原文「数据实测和」的字段和口径）。
    pub const fn new() -> SumEngine {
        SumEngine {
            primary: SumCaliber::FieldFold,
        }
    }

    /// 指定主口径引擎（口径混用演练时用 B 主复核 A 值档）。
    pub const fn with_caliber(primary: SumCaliber) -> SumEngine {
        SumEngine { primary }
    }

    /// 主口径。
    pub const fn primary(&self) -> SumCaliber {
        self.primary
    }

    /// 从读数取主口径总和。
    pub const fn primary_total(&self, reading: &SumReading) -> u16 {
        match self.primary {
            SumCaliber::FieldFold => reading.fold_total,
            SumCaliber::AsciiBytes => reading.ascii_total,
        }
    }

    /// 从读数取另一口径总和。
    pub const fn other_total(&self, reading: &SumReading) -> u16 {
        match self.primary {
            SumCaliber::FieldFold => reading.ascii_total,
            SumCaliber::AsciiBytes => reading.fold_total,
        }
    }

    /// 全量读数：110 头 + 已解析字段组 → 双口径总和 + 逐字段分账 + 归因。
    /// 头长非 110 → Err(HeadShort)；fields 与 head 的 check 字段值不要求一致
    /// （读数对 check 值不敏感——两口径均显式排除 check 位自指）。
    pub fn read_head(
        &self,
        head110: &[u8],
        fields: &[u32; 13],
    ) -> Result<SumReading, CpioCheckError> {
        if head110.len() != HEADER_LEN {
            return Err(CpioCheckError::new(V605cCode::HeadShort, 0, 0));
        }
        let mut fold_parts: [u16; 13] = [0; 13];
        let mut ascii_parts: [u16; 13] = [0; 13];
        let mut fold_acc: u32 = 0;
        // 口径 B 底账：110 头 ASCII 和含 6 字节魔数，先行入账（与
        // ascii_head_sum_16 全式严格同口径）。
        let mut ascii_acc: u32 = ascii_field_sum_8(&head110[..MAGIC_LEN]) as u32;
        let mut dominant_field: usize = 0;
        let mut dominant_gap: u32 = 0;
        let mut i: usize = 0;
        while i < 13 {
            let off = FIELD_OFFSETS[i];
            // 口径 A 分账：check 字段计 0（自指排除），其余折叠 16 位。
            let fold_part: u16 = if i == FIELD_CHECK {
                0
            } else {
                fold_u32_to_u16(fields[i])
            };
            fold_parts[i] = fold_part;
            fold_acc = fold_acc.wrapping_add(fold_part as u32);
            // 口径 B 分账：该字段 8 个 ASCII 字节和（check 位按 '0' 计）。
            let ascii_part: u16 = if i == FIELD_CHECK {
                ascii_field_sum_8(b"00000000")
            } else {
                ascii_field_sum_8(&head110[off..off + FIELD_HEX_LEN])
            };
            ascii_parts[i] = ascii_part;
            ascii_acc = ascii_acc.wrapping_add(ascii_part as u32);
            // 归因分账：|B 贡献 − A 贡献| argmax（首最大者）。
            let a32: i32 = fold_part as i32;
            let b32: i32 = ascii_part as i32;
            let gap: u32 = if b32 >= a32 {
                (b32 - a32) as u32
            } else {
                (a32 - b32) as u32
            };
            if gap > dominant_gap {
                dominant_gap = gap;
                dominant_field = i;
            }
            i += 1;
        }
        let fold_total: u16 = (fold_acc & 0xFFFF) as u16;
        let ascii_total: u16 = (ascii_acc & 0xFFFF) as u16;
        // 全式对拍：分账累加必须与全式口径一致（引擎自证，失守走 HeadShort
        // 之外的路径不可达——两式均 mod 65536 截断，恒等）。
        let delta: i32 = ascii_total as i32 - fold_total as i32;
        Ok(SumReading {
            fold_total,
            ascii_total,
            fold_parts,
            ascii_parts,
            delta_signed: delta,
            dominant_field,
            dominant_gap,
        })
    }
}

// 编译期账：贡献口径抽验（v mod 65536 = 低 16 位；高位贡献恒 0）。
const _: () = assert!(fold_u32_to_u16(0xFFFF) == 0xFFFF);
const _: () = assert!(fold_u32_to_u16(0x1_0000) == 0);
const _: () = assert!(fold_u32_to_u16(0x1234_5678) == 0x5678);
const _: () = assert!(fold_u32_to_u16(0x6553_F100) == 0xF100);

// ==========================================================================
// c_check 形态判别（0 形态 / 和值形态 / 越域形态）
// ==========================================================================

/// c_check 申报值形态（判据原文两形态 + 越域红形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckForm {
    /// 0 形态：newc "070701" 档案约定（GNU cpio 对 newc 恒写 0）——跳过比对
    /// 但计入零形态账。
    Zero,
    /// 和值形态：申报了一个 16 位域内和值（1..=0xFFFF）——进入主口径比对。
    Value(u16),
    /// 越域形态：申报值 > 0xFFFF——16 位折和域容纳不下，恒红。
    OutOfRange(u32),
}

impl CheckForm {
    /// 从 c_check 原始 32 位解析值判别形态。
    pub const fn from_raw(raw: u32) -> CheckForm {
        if raw == 0 {
            CheckForm::Zero
        } else if raw <= 0xFFFF {
            CheckForm::Value(raw as u16)
        } else {
            CheckForm::OutOfRange(raw)
        }
    }

    /// 形态中文名。
    pub const fn name(self) -> &'static str {
        match self {
            CheckForm::Zero => "0 形态(档案约定)",
            CheckForm::Value(_) => "和值形态",
            CheckForm::OutOfRange(_) => "越域形态(>0xFFFF)",
        }
    }

    /// 申报原始值（形态无关的账面值）。
    pub const fn raw(self) -> u32 {
        match self {
            CheckForm::Zero => 0,
            CheckForm::Value(v) => v as u32,
            CheckForm::OutOfRange(v) => v,
        }
    }

    /// 是否 0 形态（零形态账口径）。
    pub const fn is_zero(self) -> bool {
        match self {
            CheckForm::Zero => true,
            _ => false,
        }
    }
}

// 编译期账：形态判别闭集抽验。
const _: () = assert!(CheckForm::from_raw(0).is_zero());
const _: () = assert!(CheckForm::from_raw(1).raw() == 1);
const _: () = assert!(CheckForm::from_raw(0x1_0000).raw() == 0x1_0000);

// ==========================================================================
// 条目结论（verdict 闭集：绿二态 + 红三态）
// ==========================================================================

/// 单条目复核结论（Copy 小件——红态携带申报/双口径实测三元组供红行渲染）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryVerdict {
    /// 绿：0 形态跳过（newc 约定账；newcrc 族携 0 属违约红，另行叠加）。
    ZeroSkip {
        /// 申报原始值（恒 0，账面留痕）。
        declared_raw: u32,
    },
    /// 绿：和值形态与主口径实测和一致。
    Match {
        /// 命中口径。
        caliber: SumCaliber,
        /// 实测和（= 申报值）。
        measured: u16,
    },
    /// 红：申报值命中另一口径（口径混用——值对但尺错）。
    Crossed {
        /// 申报值。
        declared: u16,
        /// 主口径实测和。
        primary: u16,
        /// 另一口径实测和（= 申报值）。
        other: u16,
    },
    /// 红：申报值与双口径实测和均不一致。
    Mismatch {
        /// 申报值。
        declared: u16,
        /// 主口径实测和。
        primary: u16,
        /// 另一口径实测和。
        other: u16,
    },
    /// 红：申报值越出 16 位和域。
    OutOfRange {
        /// 申报原始值。
        declared_raw: u32,
    },
}

impl EntryVerdict {
    /// 结论判别位（指纹用：0..=4）。
    pub const fn id(self) -> u8 {
        match self {
            EntryVerdict::ZeroSkip { .. } => 0,
            EntryVerdict::Match { .. } => 1,
            EntryVerdict::Crossed { .. } => 2,
            EntryVerdict::Mismatch { .. } => 3,
            EntryVerdict::OutOfRange { .. } => 4,
        }
    }

    /// 是否红案（Crossed/Mismatch/OutOfRange；newcrc 携 0 违约红在记录层叠加）。
    pub const fn is_red(self) -> bool {
        match self {
            EntryVerdict::ZeroSkip { .. } => false,
            EntryVerdict::Match { .. } => false,
            EntryVerdict::Crossed { .. } => true,
            EntryVerdict::Mismatch { .. } => true,
            EntryVerdict::OutOfRange { .. } => true,
        }
    }

    /// 结论中文名。
    pub const fn label(self) -> &'static str {
        match self {
            EntryVerdict::ZeroSkip { .. } => "零形态跳过(记账)",
            EntryVerdict::Match { .. } => "和值匹配",
            EntryVerdict::Crossed { .. } => "口径混用红",
            EntryVerdict::Mismatch { .. } => "和值失配红",
            EntryVerdict::OutOfRange { .. } => "越域红",
        }
    }

    /// 结论对应的诊断码（绿态 None；红态映射 0x1FDA/0x1FD9/0x1FD8）。
    pub const fn code(self) -> Option<V605cCode> {
        match self {
            EntryVerdict::ZeroSkip { .. } => None,
            EntryVerdict::Match { .. } => None,
            EntryVerdict::Crossed { .. } => Some(V605cCode::CaliberCrossed),
            EntryVerdict::Mismatch { .. } => Some(V605cCode::CheckMismatch),
            EntryVerdict::OutOfRange { .. } => Some(V605cCode::CheckOutOfRange),
        }
    }
}

// ==========================================================================
// 事件账（CheckLedger）+ health() 面板
// ==========================================================================

/// 复核事件标签（正常事件不占诊断码；异常事件带 Option<u16> 指向码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckEventKind {
    /// 走查开始（带归档长）。
    WalkStart,
    /// 单条目走查通过（结构防线全过，进入复核）。
    WalkEntryOk,
    /// 走查拒收（结构失守——复核未开始即终止）。
    WalkRejected,
    /// 条目复核：零形态跳过（记账）。
    EntryZeroSkip,
    /// 条目复核：和值匹配。
    EntryMatched,
    /// 条目复核：和值失配红。
    EntryMismatch,
    /// 条目复核：口径混用红。
    EntryCrossedCaliber,
    /// 条目复核：越域红。
    EntryOutOfRange,
    /// 条目复核：newcrc 族携 0 违约红（规格要求和值形态）。
    EntryNewcrcZeroRed,
    /// TRAILER 收档条目入账。
    TrailerSeen,
    /// 尾后残余字节账注（残余 ≠ 0 合法但记账）。
    ResidualNote,
    /// 报告封账（带报告指纹）。
    ReportSealed,
    /// 注入演练装填（带注入类别标签）。
    InjectionArmed,
    /// 注入演练检出（红案/拒收捕获）。
    InjectionDetected,
    /// 双跑对拍开始。
    DualRunStart,
    /// 双跑对拍恒等（指纹一致）。
    DualRunEqual,
    /// 双跑对拍失配（指纹不一致——内部红）。
    DualRunDiverged,
}

impl CheckEventKind {
    /// 标签判别位（账本指纹用：0..=16，序即声明序）。
    pub const fn id(self) -> u8 {
        match self {
            CheckEventKind::WalkStart => 0,
            CheckEventKind::WalkEntryOk => 1,
            CheckEventKind::WalkRejected => 2,
            CheckEventKind::EntryZeroSkip => 3,
            CheckEventKind::EntryMatched => 4,
            CheckEventKind::EntryMismatch => 5,
            CheckEventKind::EntryCrossedCaliber => 6,
            CheckEventKind::EntryOutOfRange => 7,
            CheckEventKind::EntryNewcrcZeroRed => 8,
            CheckEventKind::TrailerSeen => 9,
            CheckEventKind::ResidualNote => 10,
            CheckEventKind::ReportSealed => 11,
            CheckEventKind::InjectionArmed => 12,
            CheckEventKind::InjectionDetected => 13,
            CheckEventKind::DualRunStart => 14,
            CheckEventKind::DualRunEqual => 15,
            CheckEventKind::DualRunDiverged => 16,
        }
    }

    /// 标签中文名。
    pub const fn label(self) -> &'static str {
        match self {
            CheckEventKind::WalkStart => "走查开始",
            CheckEventKind::WalkEntryOk => "条目走查通过",
            CheckEventKind::WalkRejected => "走查拒收",
            CheckEventKind::EntryZeroSkip => "零形态跳过记账",
            CheckEventKind::EntryMatched => "和值匹配",
            CheckEventKind::EntryMismatch => "和值失配红",
            CheckEventKind::EntryCrossedCaliber => "口径混用红",
            CheckEventKind::EntryOutOfRange => "越域红",
            CheckEventKind::EntryNewcrcZeroRed => "newcrc 携 0 违约红",
            CheckEventKind::TrailerSeen => "TRAILER 收档",
            CheckEventKind::ResidualNote => "尾后残余账注",
            CheckEventKind::ReportSealed => "报告封账",
            CheckEventKind::InjectionArmed => "注入装填",
            CheckEventKind::InjectionDetected => "注入检出",
            CheckEventKind::DualRunStart => "双跑对拍开始",
            CheckEventKind::DualRunEqual => "双跑恒等",
            CheckEventKind::DualRunDiverged => "双跑失配",
        }
    }

    /// 是否异常事件（异常事件须带诊断码）。
    pub const fn is_anomaly(self) -> bool {
        match self {
            CheckEventKind::WalkRejected
            | CheckEventKind::EntryMismatch
            | CheckEventKind::EntryCrossedCaliber
            | CheckEventKind::EntryOutOfRange
            | CheckEventKind::EntryNewcrcZeroRed
            | CheckEventKind::DualRunDiverged => true,
            _ => false,
        }
    }
}

/// 单条复核事件（注记走 String 堆——事件量大故整体 Clone）。
#[derive(Clone, Debug)]
pub struct CheckEvent {
    /// 事件标签。
    pub kind: CheckEventKind,
    /// 相关条目序号（无关联时 u32::MAX）。
    pub entry_index: u32,
    /// 相关归档内偏移（无关联时 0）。
    pub offset: u32,
    /// 事件注记（中文一行）。
    pub note: String,
    /// 异常诊断码（正常事件 None）。
    pub code: Option<u16>,
}

/// 复核事件账（Vec 堆存；封账后拒入——状态机守卫）。
#[derive(Clone, Debug)]
pub struct CheckLedger {
    events: Vec<CheckEvent>,
    sealed: bool,
}

impl CheckLedger {
    /// 空账。
    pub fn new() -> CheckLedger {
        CheckLedger {
            events: Vec::new(),
            sealed: false,
        }
    }

    /// 事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 推事件（封账后拒入返回 false——不静默丢；配合 push_checked 走
    /// StateGuard 红显性化）。
    pub fn push(&mut self, event: CheckEvent) -> bool {
        if self.sealed {
            return false;
        }
        self.events.push(event);
        true
    }

    /// 推事件（封账后 Err(StateGuard)——显性红）。
    pub fn push_checked(&mut self, event: CheckEvent) -> Result<(), CpioCheckError> {
        if self.sealed {
            return Err(CpioCheckError::new(
                V605cCode::StateGuard,
                event.entry_index,
                event.offset as usize,
            ));
        }
        self.events.push(event);
        Ok(())
    }

    /// 便捷推注记事件（无码）。
    pub fn push_note(
        &mut self,
        kind: CheckEventKind,
        entry_index: u32,
        offset: u32,
        note: &str,
    ) -> bool {
        self.push(CheckEvent {
            kind,
            entry_index,
            offset,
            note: String::from(note),
            code: None,
        })
    }

    /// 便捷推异常事件（带码）。
    pub fn push_anomaly(
        &mut self,
        code: V605cCode,
        entry_index: u32,
        offset: u32,
        note: &str,
    ) -> bool {
        self.push(CheckEvent {
            kind: CheckEventKind::WalkRejected,
            entry_index,
            offset,
            note: String::from(note),
            code: Some(code.code()),
        })
    }

    /// 全量事件只读视图。
    pub fn events(&self) -> &[CheckEvent] {
        &self.events
    }

    /// 封账（幂等：二次封账返回 false）。
    pub fn seal(&mut self) -> bool {
        if self.sealed {
            return false;
        }
        self.sealed = true;
        true
    }

    /// 账本指纹（FNV-1a：标签位+条目序+偏移+码位+注记字节——注记入指纹保证
    /// 双跑对拍能捕获事件面的任何漂移）。
    pub fn fingerprint(&self) -> u32 {
        let mut h = Fnv1a32::new();
        let mut i: usize = 0;
        while i < self.events.len() {
            let ev = &self.events[i];
            h.push_u8(ev.kind.id());
            h.push_u32le(ev.entry_index);
            h.push_u32le(ev.offset);
            match ev.code {
                Some(c) => h.push_u16le(c),
                None => h.push_u16le(0),
            }
            h.push_bytes(ev.note.as_bytes());
            i += 1;
        }
        h.finish()
    }

    /// 异常事件计数（带码事件）。
    pub fn anomaly_count(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.events.len() {
            if self.events[i].code.is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 健康面板（事件量/异常量/封账位/账本指纹）。
    pub fn health(&self) -> LedgerHealth {
        LedgerHealth {
            event_count: self.events.len() as u32,
            anomaly_count: self.anomaly_count(),
            sealed: self.sealed,
            ledger_fnv: self.fingerprint(),
        }
    }
}

/// 账本健康面板（Copy 小件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerHealth {
    /// 事件总数。
    pub event_count: u32,
    /// 异常事件数。
    pub anomaly_count: u32,
    /// 是否封账。
    pub sealed: bool,
    /// 账本指纹。
    pub ledger_fnv: u32,
}

// ==========================================================================
// F0604 遍历对接——TraverseFeed 接口声明
// ==========================================================================

/// 遍历喂入的单条目（接口声明：镜像 unx604 `CpioIterator`/`static_walk` 的
/// 条目输出形状——字段 13 元组/名字/数据窗口/跨度终点/收档位；本模块按纪律
/// 不引 crate:: 符号，故接口以结构体声明方式对接，F0604 侧输出逐字段填入
/// 即可被 `CheckVerify::verify_feed` 消费）。
#[derive(Clone, Debug)]
pub struct FeedEntry {
    /// 条目序号（0 起，含 TRAILER）。
    pub index: u32,
    /// 头 110 字节偏移（归档内）。
    pub head_off: usize,
    /// 头 110 字节拷贝（复核面自持——喂入后不再依赖原档生命周期）。
    pub head110: [u8; HEADER_LEN],
    /// 13 字段解析值组。
    pub fields: [u32; 13],
    /// 条目名（不含 NUL）。
    pub name: String,
    /// 数据区偏移。
    pub data_off: usize,
    /// 数据区长度。
    pub data_len: u32,
    /// 是否 TRAILER 收档条目。
    pub is_trailer: bool,
    /// 格式族。
    pub family: MagicFamily,
}

/// 遍历喂入面（F0604 遍历器输出的复核侧消费形态）。
///
/// 对接契约：
///   - `ok == true`：entries 条目序连续完整、TRAILER 收档（若档有）——复核面
///     逐条目开展；
///   - `ok == false`：遍历未过（`reject` 携拒收码与定位）——`verify_feed`
///     一律拒收（0x1FD7），复核不开展（「遍历不过则校验拒收」）。
#[derive(Clone, Debug)]
pub struct TraverseFeed {
    /// 遍历是否全过。
    pub ok: bool,
    /// 条目清单（ok=true 时有效；ok=false 时可为部分结果）。
    pub entries: Vec<FeedEntry>,
    /// 原档总长。
    pub archive_len: usize,
    /// 是否 TRAILER 收档。
    pub trailer_seen: bool,
    /// 尾后残余字节数。
    pub residual_bytes: usize,
    /// 拒收留痕（ok=false 时必填）。
    pub reject: Option<CpioCheckError>,
}

impl TraverseFeed {
    /// 从归档内走产喂入面（light_walk 同口径；失守时 ok=false + reject 留痕）。
    pub fn from_archive(archive: &[u8]) -> TraverseFeed {
        match light_walk(archive) {
            Ok(outcome) => {
                let mut entries: Vec<FeedEntry> = Vec::new();
                let mut i: usize = 0;
                while i < outcome.entries.len() {
                    let w = &outcome.entries[i];
                    let mut head110 = [0u8; HEADER_LEN];
                    let mut j: usize = 0;
                    while j < HEADER_LEN {
                        head110[j] = archive[w.head_off + j];
                        j += 1;
                    }
                    entries.push(FeedEntry {
                        index: w.index,
                        head_off: w.head_off,
                        head110,
                        fields: w.fields,
                        name: w.name.clone(),
                        data_off: w.data_off,
                        data_len: w.data_len,
                        is_trailer: w.is_trailer,
                        family: w.family,
                    });
                    i += 1;
                }
                TraverseFeed {
                    ok: true,
                    entries,
                    archive_len: outcome.archive_len,
                    trailer_seen: outcome.trailer_seen,
                    residual_bytes: outcome.residual_bytes,
                    reject: None,
                }
            }
            Err(err) => TraverseFeed {
                ok: false,
                entries: Vec::new(),
                archive_len: archive.len(),
                trailer_seen: false,
                residual_bytes: 0,
                reject: Some(err),
            },
        }
    }

    /// 手工拼装面（F0604 外部遍历器输出逐字段填入的对接通道；ok=false 须携
    /// reject，否则本构造拒绝——契约自查）。
    pub fn from_parts(
        ok: bool,
        entries: Vec<FeedEntry>,
        archive_len: usize,
        trailer_seen: bool,
        residual_bytes: usize,
        reject: Option<CpioCheckError>,
    ) -> Result<TraverseFeed, CpioCheckError> {
        if !ok && reject.is_none() {
            return Err(CpioCheckError::new(V605cCode::StateGuard, 0, 0));
        }
        Ok(TraverseFeed {
            ok,
            entries,
            archive_len,
            trailer_seen,
            residual_bytes,
            reject,
        })
    }

    /// 条目数。
    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }
}

// ==========================================================================
// 逐条目复核账记录（EntryCheckRecord）
// ==========================================================================

/// 单条目复核账（判据「逐条目全核」的最小账单元：条目名/形态/申报值/双口径
/// 实测和/结论/红码/注记——红行渲染与整档通过率的原料）。
#[derive(Clone, Debug)]
pub struct EntryCheckRecord {
    /// 条目序号。
    pub index: u32,
    /// 条目名。
    pub name: String,
    /// 头偏移。
    pub head_off: usize,
    /// 格式族。
    pub family: MagicFamily,
    /// 申报原始值（c_check 32 位解析值）。
    pub declared_raw: u32,
    /// 申报形态。
    pub form: CheckForm,
    /// 双口径读数。
    pub reading: SumReading,
    /// 结论。
    pub verdict: EntryVerdict,
    /// 是否红案（verdict 红，或 newcrc 携 0 违约叠加红）。
    pub red: bool,
    /// 红案诊断码（绿为 None）。
    pub red_code: Option<V605cCode>,
    /// 账注（违约叠加红等语义补充）。
    pub note: String,
}

impl EntryCheckRecord {
    /// 红行渲染（失配条目清单用——一行一定位）。
    pub fn render_line(&self) -> String {
        if self.red {
            match self.red_code {
                Some(c) => format!(
                    "[{:04X}] 条目{} {} @0x{:X} 申报 0x{:X} {} | {} | {}",
                    c.code() as u16,
                    self.index,
                    self.name,
                    self.head_off,
                    self.declared_raw,
                    self.verdict.label(),
                    self.reading.render(),
                    self.note
                ),
                None => format!(
                    "[????] 条目{} {} @0x{:X} 红案无码（内部失守）",
                    self.index, self.name, self.head_off
                ),
            }
        } else {
            format!(
                "[GREEN] 条目{} {} @0x{:X} {} 申报 0x{:X} | {}",
                self.index,
                self.name,
                self.head_off,
                self.verdict.label(),
                self.declared_raw,
                self.reading.render()
            )
        }
    }
}


// ==========================================================================
// CheckVerify 校验和复核引擎
// ==========================================================================

/// c_check 复核引擎（判据主体：逐条目全核——0 形态跳过记账 + 和值形态比对
/// mod 65536 + 失配红案定位条目；内走 light_walk 产 TraverseFeed 再消费，
/// 「遍历不过则校验拒收」双闸（内走失守直接 Err / 外喂 ok=false 拒收））。
#[derive(Clone, Debug)]
pub struct CheckVerify {
    /// 求和引擎（主口径声明）。
    sum: SumEngine,
    /// newcrc 族携 0 是否违约红（规格语义：newcrc 要求和值形态；默认开）。
    newcrc_zero_red: bool,
    /// 复核事件账。
    ledger: CheckLedger,
}

impl CheckVerify {
    /// 默认引擎（主口径 A=FieldFold，newcrc 携 0 违约红开启）。
    pub fn new() -> CheckVerify {
        CheckVerify {
            sum: SumEngine::new(),
            newcrc_zero_red: true,
            ledger: CheckLedger::new(),
        }
    }

    /// 指定主口径引擎。
    pub fn with_caliber(primary: SumCaliber) -> CheckVerify {
        CheckVerify {
            sum: SumEngine::with_caliber(primary),
            newcrc_zero_red: true,
            ledger: CheckLedger::new(),
        }
    }

    /// 主口径。
    pub const fn primary_caliber(&self) -> SumCaliber {
        self.sum.primary
    }

    /// newcrc 携 0 违约红开关（口径演练面：关闭后 newcrc 0 形态仅记账不红）。
    pub fn set_newcrc_zero_red(&mut self, on: bool) {
        self.newcrc_zero_red = on;
    }

    /// 事件账只读视图。
    pub fn ledger(&self) -> &CheckLedger {
        &self.ledger
    }

    /// 事件账所有权移交（封账面）。
    pub fn into_ledger(self) -> CheckLedger {
        self.ledger
    }

    /// 事件账健康面板。
    pub fn health(&self) -> LedgerHealth {
        self.ledger.health()
    }

    /// 单头复核（最小复核原语：110 头 + 已解析字段组 → 复核账记录；不含
    /// newcrc 违约叠加语义——族语义走 verify_feed 全式）。
    pub fn verify_head(
        &self,
        index: u32,
        head_off: usize,
        name: &str,
        head110: &[u8],
        fields: &[u32; 13],
    ) -> Result<EntryCheckRecord, CpioCheckError> {
        let reading = self.sum.read_head(head110, fields)?;
        let declared_raw = fields[FIELD_CHECK];
        let form = CheckForm::from_raw(declared_raw);
        let primary_measured = self.sum.primary_total(&reading);
        let other_measured = self.sum.other_total(&reading);
        let verdict = match form {
            CheckForm::Zero => EntryVerdict::ZeroSkip { declared_raw },
            CheckForm::Value(v) => {
                if v == primary_measured {
                    EntryVerdict::Match {
                        caliber: self.sum.primary,
                        measured: primary_measured,
                    }
                } else if v == other_measured {
                    EntryVerdict::Crossed {
                        declared: v,
                        primary: primary_measured,
                        other: other_measured,
                    }
                } else {
                    EntryVerdict::Mismatch {
                        declared: v,
                        primary: primary_measured,
                        other: other_measured,
                    }
                }
            }
            CheckForm::OutOfRange(raw) => EntryVerdict::OutOfRange {
                declared_raw: raw,
            },
        };
        let red = verdict.is_red();
        let red_code = verdict.code();
        Ok(EntryCheckRecord {
            index,
            name: String::from(name),
            head_off,
            family: MagicFamily::Newc, // 原语面无族上下文，账面按 newc 记
            declared_raw,
            form,
            reading,
            verdict,
            red,
            red_code,
            note: String::new(),
        })
    }

    /// 复核喂入面（F0604 对接主消费口）。
    ///
    /// - feed.ok=false → Err(TraversalReject @reject 定位)——遍历不过校验拒收；
    /// - feed.ok=true → 逐条目（含 TRAILER 条目）全核，产 IntegrityReport
    ///   （红案不抛错——红入账；结构错才抛错）。
    pub fn verify_feed(&mut self, feed: &TraverseFeed) -> Result<IntegrityReport, CpioCheckError> {
        if !feed.ok {
            let r = match feed.reject {
                Some(e) => e,
                None => CpioCheckError::new(V605cCode::StateGuard, 0, 0),
            };
            self.ledger.push_anomaly(
                V605cCode::TraversalReject,
                r.entry_index(),
                r.offset(),
                "遍历未过：校验账拒收（F0604 对接契约）",
            );
            return Err(CpioCheckError::new(
                V605cCode::TraversalReject,
                r.entry_index(),
                r.offset() as usize,
            ));
        }
        let mut records: Vec<EntryCheckRecord> = Vec::new();
        let mut i: usize = 0;
        while i < feed.entries.len() {
            let entry = &feed.entries[i];
            // 单头复核原语 + 族语义叠加。
            let mut record = self.verify_head(
                entry.index,
                entry.head_off,
                &entry.name,
                &entry.head110,
                &entry.fields,
            )?;
            record.family = entry.family;
            // newcrc 族携 0 违约叠加红（规格要求和值形态）。
            if entry.family.requires_value_check()
                && record.form.is_zero()
                && self.newcrc_zero_red
            {
                record.red = true;
                record.red_code = Some(V605cCode::CheckMismatch);
                record.note = String::from("newcrc(070702) 族 check 必须携和值，携 0 违约");
                self.ledger.push_anomaly(
                    V605cCode::CheckMismatch,
                    entry.index,
                    entry.head_off as u32,
                    "newcrc 携 0 违约红",
                );
            } else {
                // 事件账留痕（绿/红按结论分流）。
                let kind = if record.red {
                    match record.red_code {
                        Some(V605cCode::CaliberCrossed) => CheckEventKind::EntryCrossedCaliber,
                        Some(V605cCode::CheckOutOfRange) => CheckEventKind::EntryOutOfRange,
                        _ => CheckEventKind::EntryMismatch,
                    }
                } else if record.form.is_zero() {
                    CheckEventKind::EntryZeroSkip
                } else {
                    CheckEventKind::EntryMatched
                };
                let note = if record.red {
                    format!(
                        "条目{} {}：{}",
                        entry.index,
                        entry.name,
                        record.reading.render()
                    )
                } else if record.form.is_zero() {
                    format!("条目{} {} 零形态跳过（记账）", entry.index, entry.name)
                } else {
                    format!(
                        "条目{} {} 和值匹配 0x{:04X}",
                        entry.index,
                        entry.name,
                        self.sum.primary_total(&record.reading)
                    )
                };
                let code = if record.red {
                    match record.red_code {
                        Some(c) => Some(c.code()),
                        None => None,
                    }
                } else {
                    None
                };
                let off = record.head_off as u32;
                let _ = self.ledger.push(CheckEvent {
                    kind,
                    entry_index: entry.index,
                    offset: off,
                    note,
                    code,
                });
            }
            if entry.is_trailer {
                let _ = self.ledger.push_note(
                    CheckEventKind::TrailerSeen,
                    entry.index,
                    entry.head_off as u32,
                    "TRAILER!!! 收档条目入复核账",
                );
            }
            records.push(record);
            i += 1;
        }
        if feed.residual_bytes > 0 {
            let _ = self.ledger.push_note(
                CheckEventKind::ResidualNote,
                u32::MAX,
                feed.archive_len as u32,
                "TRAILER 后残余字节（合法但记账——详账归 F0607）",
            );
        }
        let report = IntegrityReport::compile(
            self.sum.primary,
            records,
            feed.trailer_seen,
            feed.residual_bytes,
            feed.archive_len,
        )?;
        let _ = self.ledger.push_note(
            CheckEventKind::ReportSealed,
            u32::MAX,
            0,
            "报告封账（指纹见 IntegrityReport.fnv）",
        );
        Ok(report)
    }

    /// 复核整档（内走 light_walk 产 feed 再消费——走查结构失守直接 Err 上抛
    /// 具体码；复核红案入报告不抛错）。
    pub fn verify_archive(&mut self, archive: &[u8]) -> Result<IntegrityReport, CpioCheckError> {
        let _ = self.ledger.push_note(
            CheckEventKind::WalkStart,
            u32::MAX,
            0,
            "走查开始（light_walk 结构防线全开）",
        );
        match light_walk(archive) {
            Ok(outcome) => {
                let feed = feed_from_outcome(archive, &outcome)?;
                let mut k: usize = 0;
                while k < feed.entries.len() {
                    let _ = self.ledger.push_note(
                        CheckEventKind::WalkEntryOk,
                        feed.entries[k].index,
                        feed.entries[k].head_off as u32,
                        "条目走查通过，进入 c_check 复核",
                    );
                    k += 1;
                }
                self.verify_feed(&feed)
            }
            Err(err) => {
                self.ledger.push_anomaly(
                    err.code(),
                    err.entry_index(),
                    err.offset(),
                    "走查拒收：结构失守，复核未开展",
                );
                Err(err)
            }
        }
    }
}

/// 走查产出 → 喂入面（内部转换：头窗拷贝进 FeedEntry 自持）。
fn feed_from_outcome(
    archive: &[u8],
    outcome: &WalkOutcome,
) -> Result<TraverseFeed, CpioCheckError> {
    let mut entries: Vec<FeedEntry> = Vec::new();
    let mut i: usize = 0;
    while i < outcome.entries.len() {
        let w = &outcome.entries[i];
        let mut head110 = [0u8; HEADER_LEN];
        if w.head_off + HEADER_LEN > archive.len() {
            return Err(CpioCheckError::new(V605cCode::DataOverrun, w.index, w.head_off));
        }
        let mut j: usize = 0;
        while j < HEADER_LEN {
            head110[j] = archive[w.head_off + j];
            j += 1;
        }
        entries.push(FeedEntry {
            index: w.index,
            head_off: w.head_off,
            head110,
            fields: w.fields,
            name: w.name.clone(),
            data_off: w.data_off,
            data_len: w.data_len,
            is_trailer: w.is_trailer,
            family: w.family,
        });
        i += 1;
    }
    Ok(TraverseFeed {
        ok: true,
        entries,
        archive_len: outcome.archive_len,
        trailer_seen: outcome.trailer_seen,
        residual_bytes: outcome.residual_bytes,
        reject: None,
    })
}

// ==========================================================================
// IntegrityReport 归档完整性面
// ==========================================================================

/// 整档复核报告（判据「复核账齐」主体：逐条目账 + 分账计数 + 通过率 +
/// 失配清单 + 收档账注 + 报告指纹）。
#[derive(Clone, Debug)]
pub struct IntegrityReport {
    /// 复核器版本（报告溯源）。
    pub engine_version: &'static str,
    /// 主口径声明（报告口径语义定位）。
    pub primary_caliber: SumCaliber,
    /// 逐条目复核账（序连续，含 TRAILER 条目）。
    pub records: Vec<EntryCheckRecord>,
    /// 总条目数。
    pub total_entries: u32,
    /// 绿：零形态跳过账（newc 约定形态——newcrc 违约红不计入此分账）。
    pub zero_skipped_green: u32,
    /// 绿：和值匹配账。
    pub matched_green: u32,
    /// 红：和值失配账。
    pub mismatch_red: u32,
    /// 红：口径混用账。
    pub crossed_red: u32,
    /// 红：越域账。
    pub out_of_range_red: u32,
    /// 红：newcrc 携 0 违约账。
    pub newcrc_zero_red: u32,
    /// 是否 TRAILER 收档（喂入面口径——ok=true 时恒真）。
    pub trailer_seen: bool,
    /// 尾后残余字节。
    pub residual_bytes: usize,
    /// 原档总长。
    pub archive_len: usize,
    /// 报告指纹（FNV-1a——双跑对拍底座）。
    pub fnv: u32,
}

impl IntegrityReport {
    /// 分账编译（内部守卫：分账之和 ≠ 总条目数 → Err(LedgerMiss)——复核面
    /// 自身缺陷显性化，绝不静默）。
    pub fn compile(
        primary_caliber: SumCaliber,
        records: Vec<EntryCheckRecord>,
        trailer_seen: bool,
        residual_bytes: usize,
        archive_len: usize,
    ) -> Result<IntegrityReport, CpioCheckError> {
        let total = records.len() as u32;
        let mut zero_green: u32 = 0;
        let mut matched: u32 = 0;
        let mut mismatch: u32 = 0;
        let mut crossed: u32 = 0;
        let mut range: u32 = 0;
        let mut nz_red: u32 = 0;
        let mut i: usize = 0;
        while i < records.len() {
            let r = &records[i];
            if r.red {
                match r.red_code {
                    Some(V605cCode::CheckMismatch) => {
                        // 区分普通失配与 newcrc 违约（后者 note 非空且形态 0）。
                        if r.form.is_zero() {
                            nz_red += 1;
                        } else {
                            mismatch += 1;
                        }
                    }
                    Some(V605cCode::CaliberCrossed) => crossed += 1,
                    Some(V605cCode::CheckOutOfRange) => range += 1,
                    _ => mismatch += 1, // 红案无码兜底（不可达——防御账）
                }
            } else if r.form.is_zero() {
                zero_green += 1;
            } else {
                matched += 1;
            }
            i += 1;
        }
        let counted = zero_green
            .saturating_add(matched)
            .saturating_add(mismatch)
            .saturating_add(crossed)
            .saturating_add(range)
            .saturating_add(nz_red);
        if counted != total {
            return Err(CpioCheckError::new(
                V605cCode::LedgerMiss,
                total,
                archive_len,
            ));
        }
        let fnv = Self::fingerprint_records(primary_caliber, &records);
        Ok(IntegrityReport {
            engine_version: V605C_VERSION,
            primary_caliber,
            records,
            total_entries: total,
            zero_skipped_green: zero_green,
            matched_green: matched,
            mismatch_red: mismatch,
            crossed_red: crossed,
            out_of_range_red: range,
            newcrc_zero_red: nz_red,
            trailer_seen,
            residual_bytes,
            archive_len,
            fnv,
        })
    }

    /// 报告指纹（FNV-1a：主口径位+逐条目[序/偏移/申报/双口径和/结论位/名/红位]
    /// ——报告任何语义漂移必改指纹）。
    pub fn fingerprint_records(
        primary: SumCaliber,
        records: &[EntryCheckRecord],
    ) -> u32 {
        let mut h = Fnv1a32::new();
        h.push_u8(primary.id());
        h.push_u32le(records.len() as u32);
        let mut i: usize = 0;
        while i < records.len() {
            let r = &records[i];
            h.push_u32le(r.index);
            h.push_u32le(r.head_off as u32);
            h.push_u32le(r.declared_raw);
            h.push_u16le(r.reading.fold_total);
            h.push_u16le(r.reading.ascii_total);
            h.push_u8(r.verdict.id());
            h.push_u8(r.family.id());
            h.push_u8(if r.red { 1 } else { 0 });
            h.push_bytes(r.name.as_bytes());
            i += 1;
        }
        h.finish()
    }

    /// 红案总数。
    pub fn red_count(&self) -> u32 {
        self.mismatch_red
            .saturating_add(self.crossed_red)
            .saturating_add(self.out_of_range_red)
            .saturating_add(self.newcrc_zero_red)
    }

    /// 绿案总数。
    pub fn green_count(&self) -> u32 {
        self.zero_skipped_green.saturating_add(self.matched_green)
    }

    /// 整档通过率（千分率；总 0 时按空档 0 计——空档不可达，走查已拒）。
    pub fn pass_rate_permille(&self) -> u32 {
        if self.total_entries == 0 {
            return 0;
        }
        self.green_count().saturating_mul(1000) / self.total_entries
    }

    /// 是否全绿。
    pub fn all_green(&self) -> bool {
        self.red_count() == 0 && self.total_entries > 0
    }

    /// 失配条目清单（红案序号列表，升序）。
    pub fn red_indices(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        while i < self.records.len() {
            if self.records[i].red {
                out.push(self.records[i].index);
            }
            i += 1;
        }
        out
    }

    /// 首红条目（定位面：None = 全绿）。
    pub fn first_red(&self) -> Option<u32> {
        let mut i: usize = 0;
        while i < self.records.len() {
            if self.records[i].red {
                return Some(self.records[i].index);
            }
            i += 1;
        }
        None
    }

    /// 按序号取复核账（越界 None）。
    pub fn record_at(&self, index: u32) -> Option<&EntryCheckRecord> {
        let mut i: usize = 0;
        while i < self.records.len() {
            if self.records[i].index == index {
                return Some(&self.records[i]);
            }
            i += 1;
        }
        None
    }

    /// 红行清单渲染（判据「失配红账」——逐行 [码] 条目序 名 偏移 申报/实测）。
    pub fn render_red_lines(&self) -> String {
        let mut out = String::new();
        let mut i: usize = 0;
        while i < self.records.len() {
            if self.records[i].red {
                out.push_str(&self.records[i].render_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }

    /// 整档摘要渲染（通过率面）。
    pub fn render_summary(&self) -> String {
        format!(
            "c_check 复核账：条目 {} | 绿 {}（零形态跳过 {} + 和值匹配 {}）| 红 {}（失配 {} + 口径混用 {} + 越域 {} + newcrc 违约 {}）| 通过率 {}/1000 | 收档 {} | 残余 {} B | 指纹 0x{:08X}",
            self.total_entries,
            self.green_count(),
            self.zero_skipped_green,
            self.matched_green,
            self.red_count(),
            self.mismatch_red,
            self.crossed_red,
            self.out_of_range_red,
            self.newcrc_zero_red,
            self.pass_rate_permille(),
            self.trailer_seen,
            self.residual_bytes,
            self.fnv
        )
    }
}

// ==========================================================================
// 产档面（canonical newc/newcrc 档构造——测试与注入演练的基准产线）
// ==========================================================================

/// 单条目产档 check 策略（0 形态 / 主口径和值 / 另一口径和值 / 原始值直写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckPolicy {
    /// check 写 0（newc 档案约定形态）。
    Zero,
    /// check 写口径 A 和值（字段值折和——产档器按 12 字段真算，非查表）。
    FoldSum,
    /// check 写口径 B 和值（头 ASCII 字节和，check 位按 '0' 计后真算）。
    AsciiSum,
    /// check 直写原始值（坏值注入/越域注入用）。
    Raw(u32),
}

impl CheckPolicy {
    /// 策略中文名。
    pub const fn name(self) -> &'static str {
        match self {
            CheckPolicy::Zero => "0 形态",
            CheckPolicy::FoldSum => "口径 A 和值",
            CheckPolicy::AsciiSum => "口径 B 和值",
            CheckPolicy::Raw(_) => "原始值直写",
        }
    }
}

/// 产档清单条目（名字/mode/ino/nlink/数据/族/check 策略）。
#[derive(Clone, Copy, Debug)]
pub struct ManifestEntry {
    /// 条目名（非空、无内嵌 NUL）。
    pub name: &'static str,
    /// c_mode（含类型位与权限位）。
    pub mode: u32,
    /// c_ino。
    pub ino: u32,
    /// c_nlink。
    pub nlink: u32,
    /// 数据区内容。
    pub data: &'static [u8],
    /// 格式族（newc/newcrc）。
    pub family: MagicFamily,
    /// check 策略。
    pub check: CheckPolicy,
}

/// TRAILER 收档条目（newc 族 0 形态——canonical 收档约定）。
pub const fn trailer_manifest() -> ManifestEntry {
    ManifestEntry {
        name: TRAILER_NAME,
        mode: 0,
        ino: 0,
        nlink: 1,
        data: b"",
        family: MagicFamily::Newc,
        check: CheckPolicy::Zero,
    }
}

/// 标准演练档清单（与 unx604 测试档同构：目录 + 常规文件 + 符号链接 + TRAILER
/// ——跨任务同构便于对账；check 全 0 形态）。
pub fn standard_manifest() -> Vec<ManifestEntry> {
    vec![
        ManifestEntry {
            name: "init.d",
            mode: 0o040755,
            ino: 1,
            nlink: 2,
            data: b"",
            family: MagicFamily::Newc,
            check: CheckPolicy::Zero,
        },
        ManifestEntry {
            name: "init",
            mode: 0o100755,
            ino: 2,
            nlink: 1,
            data: b"\x7fELF-fake-kernel-bytes",
            family: MagicFamily::Newc,
            check: CheckPolicy::Zero,
        },
        ManifestEntry {
            name: "init.link",
            mode: 0o120777,
            ino: 3,
            nlink: 1,
            data: b"init",
            family: MagicFamily::Newc,
            check: CheckPolicy::Zero,
        },
        trailer_manifest(),
    ]
}

/// 单条目产档（canonical 两遍法：先写 0 值头，再按策略回填 check 位）。
///
/// 名空 → NamesizeZero；名内嵌 NUL/超长 → NameNulMissing；数据超 u32 →
/// DataOverrun。
pub fn build_entry_bytes(entry: &ManifestEntry) -> Result<Vec<u8>, CpioCheckError> {
    if entry.name.is_empty() {
        return Err(CpioCheckError::new(
            V605cCode::NamesizeZero,
            0,
            FIELD_OFFSETS[FIELD_NAMESIZE],
        ));
    }
    let namesize = entry.name.len() + 1; // 名 + 终止 NUL
    if namesize > MAX_NAME_BYTES {
        return Err(CpioCheckError::new(
            V605cCode::NameNulMissing,
            0,
            0,
        ));
    }
    let data_len = match u32::try_from(entry.data.len()) {
        Ok(v) => v,
        Err(_) => {
            return Err(CpioCheckError::new(
                V605cCode::DataOverrun,
                0,
                FIELD_OFFSETS[FIELD_FILESIZE],
            ));
        }
    };
    let mut out: Vec<u8> = Vec::new();
    match entry.family {
        MagicFamily::Newc => out.extend_from_slice(&NEWC_MAGIC),
        MagicFamily::Newcrc => out.extend_from_slice(&NEWCRC_MAGIC),
    }
    let values: [u32; 13] = [
        entry.ino,
        entry.mode,
        0, // c_uid（产档面定 0）
        0, // c_gid
        entry.nlink,
        BUILD_MTIME,
        data_len,
        0, // c_devmajor
        0, // c_devminor
        0, // c_rdevmajor
        0, // c_rdevminor
        namesize as u32,
        0, // c_check（两遍法第一遍占 0）
    ];
    let mut i: usize = 0;
    while i < 13 {
        let enc = encode_hex8(values[i]);
        out.extend_from_slice(&enc);
        i += 1;
    }
    // check 策略回填（两遍法第二遍：口径和值按本头真算）。
    let check_value: u32 = match entry.check {
        CheckPolicy::Zero => 0,
        CheckPolicy::FoldSum => fold_field_sum_16(&values) as u32,
        CheckPolicy::AsciiSum => {
            let head110: &[u8] = &out[..HEADER_LEN];
            match ascii_head_sum_16(head110) {
                Ok(s) => s as u32,
                Err(e) => return Err(e),
            }
        }
        CheckPolicy::Raw(v) => v,
    };
    if !write_hex8_into(&mut out, FIELD_CHECK_OFFSET, check_value) {
        return Err(CpioCheckError::new(
            V605cCode::HexFieldBad,
            0,
            FIELD_CHECK_OFFSET,
        ));
    }
    out.extend_from_slice(entry.name.as_bytes());
    out.push(NAME_TERMINATOR);
    while out.len() % ARCHIVE_ALIGNMENT != 0 {
        out.push(0); // 名区垫（canonical 全 0 垫）
    }
    out.extend_from_slice(entry.data);
    while out.len() % ARCHIVE_ALIGNMENT != 0 {
        out.push(0); // 数据垫
    }
    Ok(out)
}

/// 整档产档（清单逐条拼接；TRAILER 由清单显式携带）。
pub fn build_archive(manifest: &[ManifestEntry]) -> Result<Vec<u8>, CpioCheckError> {
    let mut out: Vec<u8> = Vec::new();
    let mut i: usize = 0;
    while i < manifest.len() {
        let piece = build_entry_bytes(&manifest[i])?;
        out.extend_from_slice(&piece);
        i += 1;
    }
    if out.is_empty() {
        return Err(CpioCheckError::new(V605cCode::HeadShort, 0, 0));
    }
    Ok(out)
}

// ==========================================================================
// 注入面（坏 c_check 值 / 口径混用 / 遍历破坏）
// ==========================================================================

/// 定位第 index 条目头偏移（走查后取 head_off；越界 Err(DataOverrun)）。
fn head_offset_of(archive: &[u8], index: u32) -> Result<usize, CpioCheckError> {
    let outcome = light_walk(archive)?;
    let i = index as usize;
    if i >= outcome.entries.len() {
        return Err(CpioCheckError::new(
            V605cCode::DataOverrun,
            index,
            archive.len(),
        ));
    }
    Ok(outcome.entries[i].head_off)
}

/// 坏 check 值注入：第 index 条目 c_check 位直写 value（不触其他字节）。
pub fn inject_check_value(
    archive: &[u8],
    index: u32,
    value: u32,
) -> Result<Vec<u8>, CpioCheckError> {
    let head_off = head_offset_of(archive, index)?;
    let mut out: Vec<u8> = archive.to_vec();
    if !write_hex8_into(&mut out, head_off + FIELD_CHECK_OFFSET, value) {
        return Err(CpioCheckError::new(
            V605cCode::HexFieldBad,
            index,
            head_off + FIELD_CHECK_OFFSET,
        ));
    }
    Ok(out)
}

/// 口径混用注入：第 index 条目 c_check 位改写为「另一口径」和值（主口径
/// FieldFold 时写口径 B 和——值真算非查表；该条目双口径和恰相等时返回
/// Err(DataOverrun) 注明该条目不可演练混用）。
pub fn inject_caliber_swap(archive: &[u8], index: u32) -> Result<Vec<u8>, CpioCheckError> {
    let head_off = head_offset_of(archive, index)?;
    let engine = SumEngine::new(); // 主口径 A
    let head = match head_window(archive, head_off) {
        Some(h) => h,
        None => {
            return Err(CpioCheckError::new(
                V605cCode::DataOverrun,
                index,
                head_off,
            ));
        }
    };
    let fields = parse_head_at(archive, head_off)?;
    let reading = engine.read_head(head, &fields)?;
    if reading.calibers_agree() {
        return Err(CpioCheckError::new(
            V605cCode::DataOverrun,
            index,
            head_off,
        ));
    }
    let other = reading.ascii_total as u32;
    inject_check_value(archive, index, other)
}

/// 遍历破坏注入：第 index 条目魔数首字节 '0' → '9'（结构防线必拒——复核面
/// 上游走查不过演练）。
pub fn inject_magic_break(archive: &[u8], index: u32) -> Result<Vec<u8>, CpioCheckError> {
    let head_off = head_offset_of(archive, index)?;
    let mut out: Vec<u8> = archive.to_vec();
    if head_off >= out.len() {
        return Err(CpioCheckError::new(
            V605cCode::DataOverrun,
            index,
            head_off,
        ));
    }
    out[head_off] = b'9';
    Ok(out)
}

/// 注入类别（演练三类：坏值/口径混用/遍历破坏——判据「失配检出注入红」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectionKind {
    /// 坏 c_check 值注入（第 entry 条直写 bad 值）。
    BadCheckValue { entry: u32, bad: u32 },
    /// 口径混用注入（第 entry 条 check 携另一口径和）。
    CaliberSwap { entry: u32 },
    /// 遍历破坏注入（第 entry 条魔数破坏）。
    MagicBreak { entry: u32 },
}

impl InjectionKind {
    /// 类别标签。
    pub const fn label(self) -> &'static str {
        match self {
            InjectionKind::BadCheckValue { .. } => "坏 c_check 值注入",
            InjectionKind::CaliberSwap { .. } => "口径混用注入",
            InjectionKind::MagicBreak { .. } => "遍历破坏注入",
        }
    }

    /// 类别判别位（演练账指纹用）。
    pub const fn id(self) -> u8 {
        match self {
            InjectionKind::BadCheckValue { .. } => 0,
            InjectionKind::CaliberSwap { .. } => 1,
            InjectionKind::MagicBreak { .. } => 2,
        }
    }
}

/// 单演练案账（类别/检出位/定位注记）。
#[derive(Clone, Debug)]
pub struct DrillCase {
    /// 注入类别。
    pub kind: InjectionKind,
    /// 是否检出（红案捕获或拒收捕获）。
    pub detected: bool,
    /// 定位注记（检出条目序/拒收码/红码）。
    pub detail: String,
}

/// 注入演练报告（检出率账——判据要求检出 100%）。
#[derive(Clone, Debug)]
pub struct DrillReport {
    /// 逐案账。
    pub cases: Vec<DrillCase>,
    /// 检出数。
    pub detected: u32,
    /// 总案数。
    pub total: u32,
    /// 检出率（千分率）。
    pub rate_permille: u32,
    /// 演练账指纹。
    pub fnv: u32,
}

impl DrillReport {
    /// 全检出断言（检出率 < 100% → Err(InjectionUndetected)——演练面自证）。
    pub fn require_full_detection(&self) -> Result<(), CpioCheckError> {
        if self.total > 0 && self.detected == self.total {
            return Ok(());
        }
        Err(CpioCheckError::new(
            V605cCode::InjectionUndetected,
            0,
            self.total as usize,
        ))
    }
}

/// 选一条双口径和发散（fold != ascii）的非 TRAILER 条目（口径混用演练需要
/// 值可区分的条目；全部收敛时返回 None——演练面据此跳案并注明）。
pub fn pick_divergent_entry(feed: &TraverseFeed) -> Option<u32> {
    let engine = SumEngine::new();
    let mut i: usize = 0;
    while i < feed.entries.len() {
        let e = &feed.entries[i];
        if !e.is_trailer {
            if let Ok(reading) = engine.read_head(&e.head110, &e.fields) {
                if !reading.calibers_agree() {
                    return Some(e.index);
                }
            }
        }
        i += 1;
    }
    None
}

/// 注入演练（三类各一案，逐案独立引擎独立账——检出率 100% 自证）：
///   1. 坏值：取基档首条实测 fold，bad=fold+0x1337（若撞 fold/ascii 逐次
///      +0x0F0F 避让——确定性避让非随机），注入后复核须红在目标条目且
///      申报值 == bad；
///   2. 口径混用：对发散条目注入另一口径和，复核须 Crossed 红（口径归因）；
///   3. 遍历破坏：首条魔数破坏，复核须 Err 拒收（结构防线先于复核）。
pub fn run_injection_drill(base: &[u8]) -> Result<DrillReport, CpioCheckError> {
    let mut cases: Vec<DrillCase> = Vec::new();
    let mut detected: u32 = 0;
    // ---- 案一：坏 c_check 值 ----
    {
        let feed = TraverseFeed::from_archive(base);
        if !feed.ok || feed.entries.is_empty() {
            return Err(CpioCheckError::new(
                V605cCode::TraversalReject,
                0,
                0,
            ));
        }
        let target = feed.entries[0].index;
        let engine = SumEngine::new();
        let reading = engine.read_head(&feed.entries[0].head110, &feed.entries[0].fields)?;
        let mut bad: u32 = (reading.fold_total as u32).wrapping_add(0x1337) & 0xFFFF;
        // 确定性避让：bad 不得撞 fold（匹配绿）也不得撞 ascii（混用红）。
        let mut guard: u32 = 0;
        while (bad == reading.fold_total as u32 || bad == reading.ascii_total as u32)
            && guard < 8
        {
            bad = bad.wrapping_add(0x0F0F) & 0xFFFF;
            guard += 1;
        }
        let bad_archive = inject_check_value(base, target, bad)?;
        let mut verifier = CheckVerify::new();
        match verifier.verify_archive(&bad_archive) {
            Ok(report) => {
                let hit = report
                    .record_at(target)
                    .map(|r| r.red && r.declared_raw == bad)
                    .unwrap_or(false);
                cases.push(DrillCase {
                    kind: InjectionKind::BadCheckValue { entry: target, bad },
                    detected: hit,
                    detail: format!(
                        "红案条目 {}（申报 0x{:X}）| 首红 {:?}",
                        target,
                        bad,
                        report.first_red()
                    ),
                });
                if hit {
                    detected += 1;
                }
            }
            Err(e) => {
                cases.push(DrillCase {
                    kind: InjectionKind::BadCheckValue { entry: target, bad },
                    detected: false,
                    detail: format!("意外拒收：{}", e.describe()),
                });
            }
        }
    }
    // ---- 案二：口径混用 ----
    {
        let feed = TraverseFeed::from_archive(base);
        match pick_divergent_entry(&feed) {
            Some(target) => match inject_caliber_swap(base, target) {
                Ok(mixed) => {
                    let mut verifier = CheckVerify::new();
                    match verifier.verify_archive(&mixed) {
                        Ok(report) => {
                            let hit = report
                                .record_at(target)
                                .map(|r| {
                                    r.red
                                        && r.red_code == Some(V605cCode::CaliberCrossed)
                                })
                                .unwrap_or(false);
                            cases.push(DrillCase {
                                kind: InjectionKind::CaliberSwap { entry: target },
                                detected: hit,
                                detail: format!(
                                    "Crossed 红条目 {}（申报=口径 B 和）| 首红 {:?}",
                                    target,
                                    report.first_red()
                                ),
                            });
                            if hit {
                                detected += 1;
                            }
                        }
                        Err(e) => {
                            cases.push(DrillCase {
                                kind: InjectionKind::CaliberSwap { entry: target },
                                detected: false,
                                detail: format!("意外拒收：{}", e.describe()),
                            });
                        }
                    }
                }
                Err(e) => {
                    cases.push(DrillCase {
                        kind: InjectionKind::CaliberSwap { entry: target },
                        detected: false,
                        detail: format!("注入失败：{}", e.describe()),
                    });
                }
            },
            None => {
                cases.push(DrillCase {
                    kind: InjectionKind::CaliberSwap { entry: 0 },
                    detected: false,
                    detail: String::from("无可演练发散条目（基档双口径全收敛）"),
                });
            }
        }
    }
    // ---- 案三：遍历破坏 ----
    {
        let broken = inject_magic_break(base, 0)?;
        let mut verifier = CheckVerify::new();
        match verifier.verify_archive(&broken) {
            Ok(_) => {
                cases.push(DrillCase {
                    kind: InjectionKind::MagicBreak { entry: 0 },
                    detected: false,
                    detail: String::from("魔数破坏档复核意外全过（漏检）"),
                });
            }
            Err(e) => {
                let hit = matches!(
                    e.code(),
                    V605cCode::MagicMismatch
                        | V605cCode::TraversalReject
                        | V605cCode::HeadShort
                );
                cases.push(DrillCase {
                    kind: InjectionKind::MagicBreak { entry: 0 },
                    detected: hit,
                    detail: format!("拒收码 {:04X}", e.code_value()),
                });
                if hit {
                    detected += 1;
                }
            }
        }
    }
    let total = cases.len() as u32;
    let rate = if total > 0 {
        detected.saturating_mul(1000) / total
    } else {
        0
    };
    let mut h = Fnv1a32::new();
    let mut k: usize = 0;
    while k < cases.len() {
        h.push_u8(cases[k].kind.id());
        h.push_u8(if cases[k].detected { 1 } else { 0 });
        h.push_bytes(cases[k].detail.as_bytes());
        k += 1;
    }
    Ok(DrillReport {
        cases,
        detected,
        total,
        rate_permille: rate,
        fnv: h.finish(),
    })
}

// ==========================================================================
// 双跑确定性对拍
// ==========================================================================

/// 双跑对拍证明（同档两次独立引擎独立账全走——指纹恒等自证）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualRunProof {
    /// 首跑报告指纹。
    pub run_a_fnv: u32,
    /// 次跑报告指纹。
    pub run_b_fnv: u32,
    /// 首跑条目数。
    pub run_a_entries: u32,
    /// 次跑条目数。
    pub run_b_entries: u32,
    /// 首跑事件账指纹。
    pub ledger_a_fnv: u32,
    /// 次跑事件账指纹。
    pub ledger_b_fnv: u32,
    /// 是否恒等（报告指纹+条目数+账本指纹三面全等）。
    pub deterministic: bool,
}

/// 双跑确定性对拍（主入口：任何一面漂移 → Err(FingerprintMiss)）。
pub fn dual_run_determinism(archive: &[u8]) -> Result<DualRunProof, CpioCheckError> {
    let mut engine_a = CheckVerify::new();
    let report_a = engine_a.verify_archive(archive)?;
    let mut engine_b = CheckVerify::new();
    let report_b = engine_b.verify_archive(archive)?;
    let ledger_a = engine_a.ledger().fingerprint();
    let ledger_b = engine_b.ledger().fingerprint();
    let deterministic = report_a.fnv == report_b.fnv
        && report_a.total_entries == report_b.total_entries
        && ledger_a == ledger_b;
    let proof = DualRunProof {
        run_a_fnv: report_a.fnv,
        run_b_fnv: report_b.fnv,
        run_a_entries: report_a.total_entries,
        run_b_entries: report_b.total_entries,
        ledger_a_fnv: ledger_a,
        ledger_b_fnv: ledger_b,
        deterministic,
    };
    if !deterministic {
        return Err(CpioCheckError::new(
            V605cCode::FingerprintMiss,
            proof.run_b_entries,
            archive.len(),
        ));
    }
    Ok(proof)
}

/// 一次性复核便捷面（独立引擎，报告直取）。
pub fn verify_once(archive: &[u8]) -> Result<IntegrityReport, CpioCheckError> {
    let mut engine = CheckVerify::new();
    engine.verify_archive(archive)
}

// ==========================================================================
// 测试面（探针 crate 内跑：真 newc 档构造→逐条复核/坏值注入检出/口径对拍/
// 双跑一致/演练 100% 检出）
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------- 测试侧独立产档器（与产线 build_entry_bytes 异构） --------

    /// 测试侧 u32 → 8 位小写 hex 字符串（走 core::fmt，不走 encode_hex8）。
    fn hex8s(value: u32) -> String {
        format!("{:08x}", value)
    }

    /// 测试侧独立单条产档（format! 拼字段 + 逐垫对齐——与产线两遍法不同路径）。
    fn build_entry_independent(
        name: &str,
        mode: u32,
        nlink: u32,
        ino: u32,
        data: &[u8],
        family: MagicFamily,
        check_raw: u32,
    ) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        match family {
            MagicFamily::Newc => out.extend_from_slice(b"070701"),
            MagicFamily::Newcrc => out.extend_from_slice(b"070702"),
        }
        out.extend_from_slice(hex8s(ino).as_bytes()); // c_ino
        out.extend_from_slice(hex8s(mode).as_bytes()); // c_mode
        out.extend_from_slice(hex8s(0).as_bytes()); // c_uid
        out.extend_from_slice(hex8s(0).as_bytes()); // c_gid
        out.extend_from_slice(hex8s(nlink).as_bytes()); // c_nlink
        out.extend_from_slice(hex8s(BUILD_MTIME).as_bytes()); // c_mtime
        out.extend_from_slice(hex8s(data.len() as u32).as_bytes()); // c_filesize
        out.extend_from_slice(hex8s(0).as_bytes()); // c_devmajor
        out.extend_from_slice(hex8s(0).as_bytes()); // c_devminor
        out.extend_from_slice(hex8s(0).as_bytes()); // c_rdevmajor
        out.extend_from_slice(hex8s(0).as_bytes()); // c_rdevminor
        out.extend_from_slice(hex8s(name.len() as u32 + 1).as_bytes()); // c_namesize
        out.extend_from_slice(hex8s(check_raw).as_bytes()); // c_check
        assert_eq!(out.len(), HEADER_LEN, "测试产档头必须恰 110 字节");
        out.extend_from_slice(name.as_bytes());
        out.push(NAME_TERMINATOR);
        while out.len() % ARCHIVE_ALIGNMENT != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while out.len() % ARCHIVE_ALIGNMENT != 0 {
            out.push(0);
        }
        out
    }

    /// 测试侧标准好档（check 全 0 形态）。
    fn good_archive_independent() -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&build_entry_independent(
            "init.d",
            0o040755,
            2,
            1,
            b"",
            MagicFamily::Newc,
            0,
        ));
        out.extend_from_slice(&build_entry_independent(
            "init",
            0o100755,
            1,
            2,
            b"\x7fELF-fake-kernel-bytes",
            MagicFamily::Newc,
            0,
        ));
        out.extend_from_slice(&build_entry_independent(
            "init.link",
            0o120777,
            1,
            3,
            b"init",
            MagicFamily::Newc,
            0,
        ));
        out.extend_from_slice(&build_entry_independent(
            TRAILER_NAME,
            0,
            1,
            0,
            b"",
            MagicFamily::Newc,
            0,
        ));
        out
    }

    /// 期望检出码（Err 码比对；Ok 则显性测试失败）。
    fn expect_err_code<T>(result: Result<T, CpioCheckError>, want: u16) -> CpioCheckError {
        match result {
            Err(err) => {
                assert_eq!(
                    err.code_value(),
                    want,
                    "期望码 {:04X}，实得 {:04X}（{}）",
                    want,
                    err.code_value(),
                    err.describe()
                );
                err
            }
            Ok(_) => panic!("期望检出码 {:04X}，但复核意外全过", want),
        }
    }

    // ---------------- 诊断闭集账 ----------------

    #[test]
    fn diag_codes_closed_set() {
        assert_eq!(V605cCode::ALL.len(), 16);
        let mut i: usize = 0;
        while i < V605cCode::ALL.len() {
            let code = V605cCode::ALL[i];
            assert_eq!(code.code(), V605C_SEG_BASE + i as u16, "段内码须连续");
            assert_eq!(V605cCode::from_code(code.code()), Some(code), "码反查闭环");
            assert!(!code.name().is_empty());
            assert!(code.detail().chars().count() >= 16, "诊断须中文可操作");
            i += 1;
        }
        assert_eq!(V605cCode::from_code(0x1FCF), None, "段外码必须落空");
        assert_eq!(V605cCode::from_code(0x1FE0), None, "段外码必须落空");
    }

    #[test]
    fn error_accounting_fields() {
        let err = CpioCheckError::new(V605cCode::CheckMismatch, 3, 0x1234);
        assert_eq!(err.code(), V605cCode::CheckMismatch);
        assert_eq!(err.entry_index(), 3);
        assert_eq!(err.offset(), 0x1234);
        assert_eq!(err.detail(), V605cCode::CheckMismatch.detail());
        let described = err.describe();
        assert!(described.contains("1FD9"), "describe 须带段内码");
        assert!(described.contains("条目3"), "describe 须带条目定位");
        // 超域偏移收口留痕（不静默截 0）。
        let big = CpioCheckError::new(V605cCode::HeadShort, 0, 0x1_0000_0000usize);
        assert_eq!(big.offset(), u32::MAX);
    }

    // ---------------- 手写 hex 编解码 ----------------

    #[test]
    fn hex_roundtrip() {
        assert_eq!(parse_hex8(b"00000000"), Ok(0));
        assert_eq!(parse_hex8(b"ffffffff"), Ok(0xFFFF_FFFF));
        assert_eq!(parse_hex8(b"07070100"), Ok(0x0707_0100));
        assert_eq!(parse_hex8(b"ABCDEF01"), Ok(0xABCD_EF01));
        assert_eq!(parse_hex8(b"0000000g"), Err(V605cCode::HexFieldBad));
        assert_eq!(parse_hex8(b"12345"), Err(V605cCode::HexFieldBad));
        let mut i: u32 = 0;
        while i < 0x10000 {
            let enc = encode_hex8(i);
            assert_eq!(parse_hex8(&enc), Ok(i), "u16 域全量往返");
            i = i.wrapping_mul(31).wrapping_add(7); // 确定性游走覆盖
        }
        let mut buf = [0u8; 110];
        assert!(write_hex8_into(&mut buf, FIELD_CHECK_OFFSET, 0xD00D));
        assert_eq!(&buf[FIELD_CHECK_OFFSET..FIELD_CHECK_OFFSET + 8], b"0000d00d");
        assert!(!write_hex8_into(&mut buf, 106, 1), "越界写必须拒绝");
    }

    #[test]
    fn family_and_alignment() {
        assert_eq!(
            MagicFamily::from_magic(b"070701"),
            Some(MagicFamily::Newc)
        );
        assert_eq!(
            MagicFamily::from_magic(b"070702"),
            Some(MagicFamily::Newcrc)
        );
        assert_eq!(MagicFamily::from_magic(b"070703"), None);
        assert_eq!(MagicFamily::from_magic(b"0707"), None);
        assert!(MagicFamily::Newcrc.requires_value_check());
        assert!(!MagicFamily::Newc.requires_value_check());
        assert_eq!(pad4(0), 0);
        assert_eq!(pad4(3), 1);
        assert_eq!(align4(111), 112);
    }

    // ---------------- SumEngine 双口径（独立手算对拍） ----------------

    #[test]
    fn sum_engine_fold_caliber_hand_check() {
        // 折和口径：12 字段值和 mod 65536（check 计 0）——测试侧 u64 独立累加。
        let fields: [u32; 13] = [
            0xC51, 0o100755, 0, 0, 1, 1_700_000_000, 23, 0, 0, 0, 0, 6, 0,
        ];
        let mut expect: u64 = 0;
        let mut i: usize = 0;
        while i < 13 {
            if i != FIELD_CHECK {
                expect += fields[i] as u64;
            }
            i += 1;
        }
        let expect16 = (expect % 65536) as u16;
        assert_eq!(fold_field_sum_16(&fields), expect16, "折和 = 独立 u64 累加");
        // 贡献恒等式：Σ (v mod 65536) ≡ Σ v (mod 65536)——分账与全式同值。
        let mut folded: u64 = 0;
        let mut j: usize = 0;
        while j < 13 {
            folded += fold_u32_to_u16(fields[j]) as u64;
            j += 1;
        }
        assert_eq!((folded % 65536) as u16, expect16, "贡献恒等式");
    }

    #[test]
    fn sum_engine_ascii_caliber_hand_check() {
        // ASCII 口径：110 头字节和（check 8 字节按 '0' 计）mod 65536——测试侧
        // 独立 u64 累加对拍。
        let entry_bytes = build_entry_bytes(&ManifestEntry {
            name: "init",
            mode: 0o100755,
            ino: 2,
            nlink: 1,
            data: b"\x7fELF-fake-kernel-bytes",
            family: MagicFamily::Newc,
            check: CheckPolicy::Zero,
        })
        .unwrap();
        let head: &[u8] = &entry_bytes[..HEADER_LEN]; // 产线返回整条目，切出头 110
        assert_eq!(head.len(), HEADER_LEN);
        let mut expect: u64 = 0;
        let mut i: usize = 0;
        while i < HEADER_LEN {
            if i >= FIELD_CHECK_OFFSET && i < FIELD_CHECK_OFFSET + 8 {
                expect += b'0' as u64;
            } else {
                expect += head[i] as u64;
            }
            i += 1;
        }
        let expect16 = (expect % 65536) as u16;
        assert_eq!(
            ascii_head_sum_16(&head).unwrap(),
            expect16,
            "ASCII 和 = 独立 u64 累加"
        );
        // 读数面：双口径并记 + 归因字段域内 + 总差带符号自洽。
        let fields = parse_head_at(&head, 0).unwrap();
        let engine = SumEngine::new();
        let reading = engine.read_head(&head, &fields).unwrap();
        assert_eq!(reading.fold_total, fold_field_sum_16(&fields));
        assert_eq!(reading.ascii_total, expect16);
        assert_eq!(
            reading.delta_signed,
            reading.ascii_total as i32 - reading.fold_total as i32
        );
        assert!(reading.dominant_field < 13);
        // 产档头全 0 值字段（uid/gid/dev 族）ASCII 贡献恒 8×'0'=0x180。
        assert_eq!(reading.ascii_parts[2], 0x180);
        assert_eq!(reading.fold_parts[2], 0);
    }

    // ---------------- 好档全绿（0 形态跳过记账） ----------------

    #[test]
    fn good_archive_zero_form_all_green() {
        let archive = good_archive_independent();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&archive).unwrap();
        assert_eq!(report.total_entries, 4, "3 条目 + TRAILER");
        assert!(report.all_green());
        assert_eq!(report.zero_skipped_green, 4, "全 0 形态跳过记账");
        assert_eq!(report.matched_green, 0);
        assert_eq!(report.red_count(), 0);
        assert_eq!(report.pass_rate_permille(), 1000);
        assert!(report.trailer_seen);
        assert_eq!(report.residual_bytes, 0);
        assert_eq!(report.first_red(), None);
        // 事件账健康：零异常。
        let health = engine.health();
        assert_eq!(health.anomaly_count, 0);
        assert!(health.event_count >= 4);
    }

    #[test]
    fn value_form_archive_matched_green() {
        // 和值形态档：check 携口径 A 折和（产线真算）→ 复核全匹配绿。
        let mut manifest = standard_manifest();
        let mut i: usize = 0;
        while i < manifest.len() {
            manifest[i].check = CheckPolicy::FoldSum;
            i += 1;
        }
        let archive = build_archive(&manifest).unwrap();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&archive).unwrap();
        assert!(report.all_green());
        assert_eq!(report.matched_green, 4, "和值形态逐条匹配");
        assert_eq!(report.zero_skipped_green, 0);
        assert_eq!(report.pass_rate_permille(), 1000);
        // 逐条记录结论核验。
        let mut k: usize = 0;
        while k < report.records.len() {
            let r = &report.records[k];
            assert!(!r.form.is_zero(), "和值形态档不应有 0 形态条目");
            match r.verdict {
                EntryVerdict::Match { measured, .. } => {
                    assert_eq!(measured as u32, r.declared_raw);
                }
                _ => panic!("和值形态档全匹配：条目{} 结论 {:?}", r.index, r.verdict),
            }
            k += 1;
        }
    }

    // ---------------- 坏 c_check 值注入：检出 + 定位条目 ----------------

    #[test]
    fn bad_check_value_injection_detected() {
        let archive = good_archive_independent();
        // 从好档复核账取条目 1（init）双口径实测和，构造确定性坏值。
        let mut probe = CheckVerify::new();
        let base_report = probe.verify_archive(&archive).unwrap();
        let rec = base_report.record_at(1).unwrap();
        let fold = rec.reading.fold_total as u32;
        let ascii = rec.reading.ascii_total as u32;
        let mut bad: u32 = fold.wrapping_add(0x1337) & 0xFFFF;
        let mut guard: u32 = 0;
        while (bad == fold || bad == ascii) && guard < 8 {
            bad = bad.wrapping_add(0x0F0F) & 0xFFFF;
            guard += 1;
        }
        assert!(bad != fold && bad != ascii, "坏值须双口径均不命中");
        let bad_archive = inject_check_value(&archive, 1, bad).unwrap();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&bad_archive).unwrap();
        assert!(!report.all_green());
        assert_eq!(report.first_red(), Some(1), "失配定位到条目 1");
        assert_eq!(report.red_indices(), vec![1], "仅目标条目红");
        let red = report.record_at(1).unwrap();
        assert!(red.red);
        assert_eq!(red.red_code, Some(V605cCode::CheckMismatch));
        assert_eq!(red.declared_raw, bad, "申报值即注入坏值");
        assert_eq!(report.pass_rate_permille(), 750, "4 条目 1 红 → 3/4");
        let lines = report.render_red_lines();
        assert!(lines.contains("条目1"), "红行须定位条目序");
        assert!(lines.contains("init"), "红行须定位条目名");
        assert!(lines.contains("1FD9"), "红行须带诊断码");
        // 事件账异常面。
        let health = engine.health();
        assert!(health.anomaly_count >= 1);
    }

    // ---------------- 口径混用注入：Crossed 红归因 ----------------

    #[test]
    fn caliber_swap_injection_crossed_red() {
        let archive = good_archive_independent();
        let target = {
            let feed = TraverseFeed::from_archive(&archive);
            pick_divergent_entry(&feed).expect("标准档必有发散条目")
        };
        let mixed = inject_caliber_swap(&archive, target).unwrap();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&mixed).unwrap();
        let rec = report.record_at(target).unwrap();
        assert!(rec.red, "口径混用须红");
        assert_eq!(rec.red_code, Some(V605cCode::CaliberCrossed));
        match rec.verdict {
            EntryVerdict::Crossed { declared, primary, other } => {
                assert_eq!(declared, other, "申报值恰为另一口径和");
                assert_ne!(declared, primary);
            }
            _ => panic!("口径混用结论须 Crossed"),
        }
        // 主口径换位对拍：口径 B 主引擎复核同一混用档 → 变绿（口径声明一致）。
        let mut engine_b = CheckVerify::with_caliber(SumCaliber::AsciiBytes);
        let report_b = engine_b.verify_archive(&mixed).unwrap();
        assert!(report_b.all_green(), "口径 B 主引擎下口径 B 值档应全绿");
    }

    // ---------------- 越域注入：OutOfRange 恒红 ----------------

    #[test]
    fn out_of_range_check_red() {
        let archive = good_archive_independent();
        let bad = inject_check_value(&archive, 2, 0x1_0000).unwrap();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&bad).unwrap();
        let rec = report.record_at(2).unwrap();
        assert!(rec.red);
        assert_eq!(rec.red_code, Some(V605cCode::CheckOutOfRange));
        assert_eq!(report.out_of_range_red, 1);
        assert_eq!(rec.form, CheckForm::OutOfRange(0x1_0000));
    }

    // ---------------- 遍历不过：校验拒收（双闸） ----------------

    #[test]
    fn traversal_broken_rejected() {
        let archive = good_archive_independent();
        let broken = inject_magic_break(&archive, 0).unwrap();
        // 闸一：内走直接 Err 结构码。
        let mut engine = CheckVerify::new();
        let err = expect_err_code(engine.verify_archive(&broken), 0x1FD1);
        assert_eq!(err.entry_index(), 0);
        // 闸二：外部喂入 ok=false → verify_feed 拒收 TraversalReject。
        let feed = TraverseFeed::from_archive(&broken);
        assert!(!feed.ok);
        let mut engine2 = CheckVerify::new();
        expect_err_code(engine2.verify_feed(&feed), 0x1FD7);
        // from_parts 契约：ok=false 无 reject → StateGuard。
        let bad_feed = TraverseFeed::from_parts(false, Vec::new(), 0, false, 0, None).unwrap_err();
        assert_eq!(bad_feed.code_value(), 0x1FDF);
    }

    #[test]
    fn trailer_missing_rejected() {
        // 无收档条目档：走查耗尽 → TrailerMissing（复核未开展）。
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&build_entry_independent(
            "init",
            0o100755,
            1,
            2,
            b"data",
            MagicFamily::Newc,
            0,
        ));
        let mut engine = CheckVerify::new();
        expect_err_code(engine.verify_archive(&out), 0x1FDB);
    }

    #[test]
    fn residual_after_trailer_accounted() {
        // TRAILER 后残余字节：合法但记账（非红）。
        let mut archive = good_archive_independent();
        archive.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&archive).unwrap();
        assert_eq!(report.residual_bytes, 3);
        assert!(report.all_green(), "残余非红——详账归 F0607");
        let mut seen = false;
        let evs = engine.ledger().events();
        let mut i: usize = 0;
        while i < evs.len() {
            if evs[i].kind == CheckEventKind::ResidualNote {
                seen = true;
            }
            i += 1;
        }
        assert!(seen, "残余事件须入账");
    }

    // ---------------- newcrc 族语义 ----------------

    #[test]
    fn newcrc_family_semantics() {
        // newcrc + FoldSum：和值形态匹配绿。
        let manifest_ok = vec![
            ManifestEntry {
                name: "crc_file",
                mode: 0o100644,
                ino: 9,
                nlink: 1,
                data: b"crc-family-probe",
                family: MagicFamily::Newcrc,
                check: CheckPolicy::FoldSum,
            },
            trailer_manifest(),
        ];
        let archive_ok = build_archive(&manifest_ok).unwrap();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&archive_ok).unwrap();
        assert!(report.all_green());
        assert_eq!(report.matched_green, 1);
        assert_eq!(report.zero_skipped_green, 1, "TRAILER 0 形态照常跳过");
        // newcrc + Zero：携 0 违约红（规格要求和值形态）。
        let manifest_bad = vec![
            ManifestEntry {
                name: "crc_file",
                mode: 0o100644,
                ino: 9,
                nlink: 1,
                data: b"crc-family-probe",
                family: MagicFamily::Newcrc,
                check: CheckPolicy::Zero,
            },
            trailer_manifest(),
        ];
        let archive_bad = build_archive(&manifest_bad).unwrap();
        let mut engine2 = CheckVerify::new();
        let report2 = engine2.verify_archive(&archive_bad).unwrap();
        assert_eq!(report2.newcrc_zero_red, 1);
        assert_eq!(report2.first_red(), Some(0));
        let rec = report2.record_at(0).unwrap();
        assert_eq!(rec.red_code, Some(V605cCode::CheckMismatch));
        assert!(rec.note.contains("newcrc"), "违约注记须点名族");
        // 口径演练面：关违约红开关后同档仅记账不红。
        let mut engine3 = CheckVerify::new();
        engine3.set_newcrc_zero_red(false);
        let report3 = engine3.verify_archive(&archive_bad).unwrap();
        assert!(report3.all_green());
    }

    // ---------------- 注入演练：检出率 100% 自证 ----------------

    #[test]
    fn injection_drill_full_detection() {
        let archive = good_archive_independent();
        let report = run_injection_drill(&archive).unwrap();
        assert_eq!(report.total, 3, "三类注入各一案");
        assert_eq!(report.detected, 3, "检出 3/3 = 100%");
        assert_eq!(report.rate_permille, 1000);
        report.require_full_detection().unwrap();
        // 逐案类别覆盖。
        assert_eq!(report.cases[0].kind.id(), 0, "案一坏值");
        assert_eq!(report.cases[1].kind.id(), 1, "案二口径混用");
        assert_eq!(report.cases[2].kind.id(), 2, "案三遍历破坏");
        assert!(report.cases[0].detail.contains("条目 0"));
    }

    // ---------------- 双跑确定性对拍 ----------------

    #[test]
    fn dual_run_deterministic() {
        let archive = good_archive_independent();
        let proof = dual_run_determinism(&archive).unwrap();
        assert!(proof.deterministic);
        assert_eq!(proof.run_a_fnv, proof.run_b_fnv);
        assert_eq!(proof.ledger_a_fnv, proof.ledger_b_fnv);
        assert_eq!(proof.run_a_entries, 4);
        // 混入坏值后双跑仍须确定性（红案路径同样恒等）。
        let bad = inject_check_value(&archive, 1, 0x777).unwrap();
        let proof2 = dual_run_determinism(&bad).unwrap();
        assert!(proof2.deterministic, "红案路径双跑恒等");
        // 报告指纹对坏值敏感：注入前后指纹必变。
        let good_report = verify_once(&archive).unwrap();
        let bad_report = verify_once(&bad).unwrap();
        assert_ne!(good_report.fnv, bad_report.fnv, "指纹须捕获 check 漂移");
    }

    // ---------------- 整档账数学 + 渲染面 ----------------

    #[test]
    fn integrity_accounting_math() {
        let archive = good_archive_independent();
        let report = verify_once(&archive).unwrap();
        // 分账守恒：绿 + 红 == 总条目。
        assert_eq!(
            report.green_count() + report.red_count(),
            report.total_entries
        );
        // 摘要渲染面要素齐。
        let summary = report.render_summary();
        assert!(summary.contains("通过率 1000/1000"));
        assert!(summary.contains("条目 4"));
        assert!(summary.contains("收档 true"));
        // 红行渲染：好档空。
        assert_eq!(report.render_red_lines(), "");
        // 单头复核原语：与整档口径一致。
        let fields = parse_head_at(&archive, 0).unwrap();
        let head = head_window(&archive, 0).unwrap();
        let engine = CheckVerify::new();
        let rec = engine.verify_head(0, 0, "init.d", head, &fields).unwrap();
        assert!(!rec.red);
        assert_eq!(rec.form, CheckForm::Zero);
        // 指纹确定性：同输入两次编译指纹恒等。
        let f1 = IntegrityReport::fingerprint_records(SumCaliber::FieldFold, &report.records);
        let f2 = IntegrityReport::fingerprint_records(SumCaliber::FieldFold, &report.records);
        assert_eq!(f1, f2);
        assert_eq!(f1, report.fnv);
    }

    // ---------------- 账本状态机守卫 ----------------

    #[test]
    fn ledger_seal_state_guard() {
        let mut ledger = CheckLedger::new();
        assert!(ledger.push_note(CheckEventKind::WalkStart, u32::MAX, 0, "开账"));
        assert!(ledger.seal());
        assert!(!ledger.seal(), "二次封账幂等拒绝");
        assert!(!ledger.push_note(CheckEventKind::WalkStart, u32::MAX, 0, "封后再入"));
        let err = ledger
            .push_checked(CheckEvent {
                kind: CheckEventKind::WalkStart,
                entry_index: 0,
                offset: 0,
                note: String::from("封后再入"),
                code: None,
            })
            .unwrap_err();
        assert_eq!(err.code_value(), 0x1FDF, "封账后再入 → StateGuard");
        assert!(ledger.is_sealed());
        assert_eq!(ledger.len(), 1, "封账后事件不增");
    }

    // ---------------- 产线/测试侧异构产档对拍 ----------------

    #[test]
    fn builder_cross_check() {
        // 产线 build_archive 与测试侧独立产档器对同清单逐字节等值
        // （异构实现往返一致——canonical 产档可信）。
        let prod = build_archive(&standard_manifest()).unwrap();
        let indep = good_archive_independent();
        assert_eq!(prod.len(), indep.len());
        let mut i: usize = 0;
        while i < prod.len() {
            assert_eq!(prod[i], indep[i], "偏移 {} 产档分叉", i);
            i += 1;
        }
        // 产档自证：口径 A 策略档的 check 位恰等于该头折和。
        let mut manifest = standard_manifest();
        manifest[0].check = CheckPolicy::FoldSum;
        let arch = build_archive(&manifest).unwrap();
        let fields = parse_head_at(&arch, 0).unwrap();
        let mut f2 = fields;
        f2[FIELD_CHECK] = 0;
        assert_eq!(fields[FIELD_CHECK], fold_field_sum_16(&f2) as u32);
    }

    // ---------------- 走查防线抽验（复核前置结构红） ----------------

    #[test]
    fn walk_guards_reachability() {
        // 空档：HeadShort。
        let mut engine = CheckVerify::new();
        expect_err_code(engine.verify_archive(&[]), 0x1FD0);
        // 短头：截 50 字节。
        let short: Vec<u8> = good_archive_independent()[..50].to_vec();
        expect_err_code(engine.verify_archive(&short), 0x1FD0);
        // namesize=0：手改首头 namesize 位（偏移 94）。
        let mut bad_ns = good_archive_independent();
        assert!(write_hex8_into(&mut bad_ns, 94, 0));
        expect_err_code(engine.verify_archive(&bad_ns), 0x1FD3);
        // 名区 NUL 破坏：namesize 由 7 改 6（"init.d" 名区失 NUL）。
        let mut bad_nul = good_archive_independent();
        assert!(write_hex8_into(&mut bad_nul, 94, 6));
        expect_err_code(engine.verify_archive(&bad_nul), 0x1FD4);
        // hex 域破坏：check 位写非 hex。
        let mut bad_hex = good_archive_independent();
        bad_hex[FIELD_CHECK_OFFSET] = b'z';
        expect_err_code(engine.verify_archive(&bad_hex), 0x1FD2);
        // 数据垫破坏：TRAILER 数据垫首字节（档末-1）置非 0。
        let mut bad_pad = good_archive_independent();
        let last = bad_pad.len() - 1;
        bad_pad[last] = 1;
        expect_err_code(engine.verify_archive(&bad_pad), 0x1FD6);
    }

    // ---------------- health() 汇总面 ----------------

    #[test]
    fn verify_health_surface() {
        let archive = good_archive_independent();
        let mut engine = CheckVerify::new();
        let report = engine.verify_archive(&archive).unwrap();
        let health = engine.health();
        assert_eq!(health.event_count, engine.ledger().len() as u32);
        assert_eq!(health.anomaly_count, 0);
        // 账本指纹独立对拍：另起引擎走同档，账本指纹须恒等；报告指纹另核。
        assert_eq!(health.ledger_fnv, engine_ledger_independent_fnv(&archive));
        assert_ne!(
            health.ledger_fnv,
            report.fnv,
            "账本指纹与报告指纹是两个独立指纹面"
        );
        // 引擎账只读视图与事件计数一致。
        assert!(!engine.ledger().is_empty());
    }

    /// 独立账面指纹（health 对拍面：另起引擎走同档，账本指纹须恒等）。
    fn engine_ledger_independent_fnv(archive: &[u8]) -> u32 {
        let mut other = CheckVerify::new();
        let _ = other.verify_archive(archive).unwrap();
        other.ledger().fingerprint()
    }
}
