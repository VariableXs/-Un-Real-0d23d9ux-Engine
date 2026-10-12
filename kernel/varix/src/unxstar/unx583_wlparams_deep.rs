//! UNX-F0583 · 白名单参数表（A1 认可集）（CoRun·Unxreal · UNX-A1-B10 · 深化并存版）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0583`
//!
//! **判据 J1**：白名单表冻结（24 键三型分布），白名单外拒收率 100%（注入 6/6 拒）。
//!
//! **并存声明**：同目录 `unx583_wlparams.rs` 为并行工人速写版（0x5210 段）；
//! 本文件为深化并存版，独立模块 `unx583_wlparams_deep`，诊断码独占
//! `0x9E01..=0x9E0F` 段（全域 grep 确认空闲，0x9E000 仅内存地址噪声）。
//! 两版互不 `use`，判据各自独立闭环。
//!
//! **要点一 · 24 键三型冻结**：布尔 8 / 整数 10 / 字符串 6，每键五元组
//! （键名/类型/值域/默认值/语义注记）。键名来源声明为 **A1 认可集·白名单
//! 参数冻结草案 UNX-F0583/WLD-24 v1**；键名统一 `unx.kernel.` 前缀——与
//! Linux `kernel.*`、systemd、GRUB 等常见同名无冲突。冻结指纹 FNV1a-64
//! 全表哈希（键名+类型+值域+字符集+默认值逐键卷入）。
//!
//! **要点二 · 冻结语义**：两态状态机 `Draft`（可变更，逐笔入变更账）→
//! `Frozen`（指纹落账；此后 set/insert/remove 全拒 + ADR 拒绝留痕立案）。
//! 冻结前置：恰 24 键、分布 8/10/6、键名无重复，任一不满足拒签。
//!
//! **要点三 · 校验器**：布尔严格集 `true/false/0/1/yes/no`（大小写敏感
//! 不归一）；整数手写 u64 解析 + 钳外拒；字符串长度上下限 + 六类字符
//! 白名单；类型不符显性计数不静默转换；白名单外键一律 KEY_UNKNOWN。
//!
//! **要点四 · 注入面与拒收率**：六样本闭集（超范围整数/非法布尔/超长
//! 字符串/路径穿越串/控制字符注入/类型混淆）6/6 全拒对拍账；拒收率
//! 千分位 `reject*1000/injected` 恰值 1000 才判达标。
//!
//! **要点五 · 边界与导出**：整数键 min/max 收、min-1/max+1 拒逐键对拍；
//! 布尔键合法 token 收、集外 token 拒；字符串键 max 长收、max+1 拒、
//! 零长拒。kv + TLV 双形态导出，TLV 回读重建指纹往返比对；查询 API
//! 三件套（存在性/类型安全取值/默认值回退带账注）。
//!
//! **纪律**：零 unwrap/expect/panic、零 IO、零墙钟、零浮点——逻辑 tick
//! 由调用方 u64 传入，全确定性可重放。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（独占 0x9E01..=0x9E0F 段）
// ---------------------------------------------------------------------------

/// 白名单参数表诊断码。独占 `0x9E01..=0x9E0F` 段（全域 grep 确认空闲，
/// 先入库者占段；与并行版 0x5210 段、同目录 0x07/0x2E/0x2F 等互不重叠）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WlCode(pub u16);

impl WlCode {
    /// 0x9E01 白名单外键（A1 认可集 24 键之外——闭集外即拒）。
    pub const KEY_UNKNOWN: WlCode = WlCode(0x9E01);
    /// 0x9E02 类型不符（布尔键喂串/整数键喂串等——显性计数不静默转）。
    pub const TYPE_MISMATCH: WlCode = WlCode(0x9E02);
    /// 0x9E03 布尔解析失败（严格集 true/false/0/1/yes/no 之外）。
    pub const BOOL_PARSE_BAD: WlCode = WlCode(0x9E03);
    /// 0x9E04 整数越域（钳外拒：min-1 / max+1 皆拒）。
    pub const INT_RANGE_OUT: WlCode = WlCode(0x9E04);
    /// 0x9E05 字符串超长（超 str_max 上限——超长注入样本面）。
    pub const STR_TOO_LONG: WlCode = WlCode(0x9E05);
    /// 0x9E06 字符串字符非法（字符白名单外 / 控制字符 / 路径穿越）。
    pub const STR_CHAR_ILLEGAL: WlCode = WlCode(0x9E06);
    /// 0x9E07 注入样本拒收（六样本闭集——拒是正确行为，此码用于对拍账留痕）。
    pub const INJECT_REJECTED: WlCode = WlCode(0x9E07);
    /// 0x9E08 表已冻结（冻结后一切写入尝试全拒——冻结语义主码）。
    pub const TABLE_FROZEN: WlCode = WlCode(0x9E08);
    /// 0x9E09 指纹漂移（TLV 回读重建指纹 ≠ 冻结指纹）。
    pub const FINGERPRINT_DRIFT: WlCode = WlCode(0x9E09);
    /// 0x9E0A TLV 导出/回读结构坏（tag 越段 / len 越面 / 记录数不符）。
    pub const EXPORT_TLV_BAD: WlCode = WlCode(0x9E0A);
    /// 0x9E0B 账目缺口（变更账/注入账/对拍账行数与操作数失平）。
    pub const LEDGER_GAP: WlCode = WlCode(0x9E0B);
    /// 0x9E0C 边界对拍不符（三值边界任一键红——值域契约破坏）。
    pub const BOUNDARY_MISMATCH: WlCode = WlCode(0x9E0C);
    /// 0x9E0D 冻结前置不满足（非 24 键 / 分布非 8-10-6 / 键名重复）。
    pub const FREEZE_INCOMPLETE: WlCode = WlCode(0x9E0D);
    /// 0x9E0E 拒收率千分位未达标（≠1000‰）。
    pub const RATE_BELOW_THOUSAND: WlCode = WlCode(0x9E0E);
    /// 0x9E0F ADR 流程必需（冻结后变更请求未走 ADR 留痕——流程闸）。
    pub const ADR_REQUIRED: WlCode = WlCode(0x9E0F);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 段位合法性（码值必须落在 0x9E01..=0x9E0F 闭集内）。
    pub const fn in_segment(raw: u16) -> bool {
        raw >= 0x9E01 && raw <= 0x9E0F
    }

    /// 人话原因（中文诊断——逐码可审计）。
    pub fn reason(self) -> String {
        match self {
            WlCode::KEY_UNKNOWN => "白名单外键：A1 认可集 24 键之外，闭集外即拒".into(),
            WlCode::TYPE_MISMATCH => "类型不符：喂入类型与键声明三型不一致，显性计数不静默转换".into(),
            WlCode::BOOL_PARSE_BAD => "布尔解析失败：token 不在严格集 true/false/0/1/yes/no".into(),
            WlCode::INT_RANGE_OUT => "整数越域：钳外拒（低于 min 或高于 max）".into(),
            WlCode::STR_TOO_LONG => "字符串超长：超出键 str_max 上限".into(),
            WlCode::STR_CHAR_ILLEGAL => "字符串字符非法：字符白名单外（含控制字符/路径穿越形态）".into(),
            WlCode::INJECT_REJECTED => "注入样本拒收：六样本闭集成员被校验器正确拒绝（对拍账留痕）".into(),
            WlCode::TABLE_FROZEN => "表已冻结：冻结后 set/insert/remove 一律全拒".into(),
            WlCode::FINGERPRINT_DRIFT => "指纹漂移：回读重建指纹与冻结指纹不一致".into(),
            WlCode::EXPORT_TLV_BAD => "TLV 结构坏：tag 越段/长度越面/记录数不符".into(),
            WlCode::LEDGER_GAP => "账目缺口：账行数与操作数失平（黑匣子漏账）".into(),
            WlCode::BOUNDARY_MISMATCH => "边界对拍不符：三值边界测试任一键红".into(),
            WlCode::FREEZE_INCOMPLETE => "冻结前置不满足：非 24 键或分布非 8-10-6 或键名重复".into(),
            WlCode::RATE_BELOW_THOUSAND => "拒收率未达标：千分位 ≠ 1000‰".into(),
            WlCode::ADR_REQUIRED => "ADR 流程必需：冻结后变更请求须留痕立案后方可裁决".into(),
            WlCode(_) => "未知 UNX 白名单参数域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// §1 错误体（Err + 中文诊断，零 panic 面）
// ---------------------------------------------------------------------------

/// 白名单域错误体：诊断码 + 逻辑 tick + 中文细节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlErr {
    /// 诊断码（0x9E 段闭集成员）。
    pub code: WlCode,
    /// 逻辑 tick（调用方传入——零墙钟，全确定性）。
    pub tick: u64,
    /// 中文细节（键名/喂入值摘要等，可审计）。
    pub detail: String,
}

impl WlErr {
    /// 快速构造。
    pub fn new(code: WlCode, tick: u64, detail: &str) -> Self {
        WlErr {
            code,
            tick,
            detail: detail.to_string(),
        }
    }

    /// 单行诊断串（`[0x9Exx] tick#N：细节 —— 人话原因`）。
    pub fn render(&self) -> String {
        format!(
            "[0x{:04X}] tick#{}：{} —— {}",
            self.code.code(),
            self.tick,
            self.detail,
            self.code.reason()
        )
    }
}

// ---------------------------------------------------------------------------
// §2 FNV1a-64 哈希（指纹底座——确定性、无浮点、无随机）
// ---------------------------------------------------------------------------

/// FNV1a-64 偏移基。
pub const FNV1A_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV1a-64 素数。
pub const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单字节卷入。
pub const fn fnv1a_byte(acc: u64, b: u8) -> u64 {
    (acc ^ (b as u64)).wrapping_mul(FNV1A_PRIME)
}

/// 字节串卷入。
pub const fn fnv1a_bytes(acc: u64, bs: &[u8]) -> u64 {
    let mut acc = acc;
    let mut i = 0;
    while i < bs.len() {
        acc = fnv1a_byte(acc, bs[i]);
        i += 1;
    }
    acc
}

/// u64 小端 8 字节卷入（指纹数值域定序——不依赖平台字节序习惯漂移）。
pub const fn fnv1a_u64(acc: u64, v: u64) -> u64 {
    let bytes = v.to_le_bytes();
    fnv1a_bytes(acc, &bytes)
}

// ---------------------------------------------------------------------------
// §3 三型与参数值
// ---------------------------------------------------------------------------

/// A1 认可集三型闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// 布尔型（严格集解析）。
    Bool,
    /// 整数型（u64 值域钳）。
    Int,
    /// 字符串型（长度 + 字符白名单）。
    Str,
}

impl ParamType {
    /// 判别值（TLV wire / 指纹卷入用——闭集 1/2/3）。
    pub const fn disc(self) -> u8 {
        match self {
            ParamType::Bool => 1,
            ParamType::Int => 2,
            ParamType::Str => 3,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            ParamType::Bool => "bool",
            ParamType::Int => "int",
            ParamType::Str => "str",
        }
    }

    /// 判别值反解（TLV 回读用——3 之外 None）。
    pub const fn of_disc(d: u8) -> Option<ParamType> {
        match d {
            1 => Some(ParamType::Bool),
            2 => Some(ParamType::Int),
            3 => Some(ParamType::Str),
            _ => None,
        }
    }
}

/// 参数运行值（三型闭集——无第四型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    /// 布尔值。
    B(bool),
    /// 整数值（u64 全域，键值域另钳）。
    I(u64),
    /// 字符串值（alloc String——no_std + alloc 纪律）。
    S(String),
}

impl ParamValue {
    /// 类型判别。
    pub const fn ty(&self) -> ParamType {
        match self {
            ParamValue::B(_) => ParamType::Bool,
            ParamValue::I(_) => ParamType::Int,
            ParamValue::S(_) => ParamType::Str,
        }
    }

    /// 字符串化（kv 导出形态——三型统一文本面）。
    pub fn to_kv_text(&self) -> String {
        match self {
            ParamValue::B(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            ParamValue::I(v) => format!("{}", v),
            ParamValue::S(s) => s.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// §4 字符白名单六类闭集
// ---------------------------------------------------------------------------

/// 字符白名单类别（六类闭集——类别外字符一律 0x9E06）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// 路径安全类：A-Z a-z 0-9 与 `/ . _ -`（禁 `..` 序列另闸）。
    PathSafe,
    /// 主机/设备标签类：A-Z a-z 0-9 与 `-`。
    TagSafe,
    /// OEM 标签类：A-Z a-z 0-9 与 `_`。
    OemSafe,
    /// 设施名类：A-Z a-z 0-9 与 `.`（如 klog.core）。
    FacSafe,
    /// 启动参数键类：A-Z a-z 0-9 与 `. _ -`。
    ArgSafe,
    /// 十六进制标签类：A-Z a-z 0-9。
    HexSafe,
}

impl Charset {
    /// 判别值（指纹/TLV 卷入）。
    pub const fn disc(self) -> u8 {
        match self {
            Charset::PathSafe => 1,
            Charset::TagSafe => 2,
            Charset::OemSafe => 3,
            Charset::FacSafe => 4,
            Charset::ArgSafe => 5,
            Charset::HexSafe => 6,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Charset::PathSafe => "path-safe[A-Za-z0-9/._-]",
            Charset::TagSafe => "tag-safe[A-Za-z0-9-]",
            Charset::OemSafe => "oem-safe[A-Za-z0-9_]",
            Charset::FacSafe => "fac-safe[A-Za-z0-9.]",
            Charset::ArgSafe => "arg-safe[A-Za-z0-9._-]",
            Charset::HexSafe => "hex-safe[A-Za-z0-9]",
        }
    }

    /// 判别值反解。
    pub const fn of_disc(d: u8) -> Option<Charset> {
        match d {
            1 => Some(Charset::PathSafe),
            2 => Some(Charset::TagSafe),
            3 => Some(Charset::OemSafe),
            4 => Some(Charset::FacSafe),
            5 => Some(Charset::ArgSafe),
            6 => Some(Charset::HexSafe),
            _ => None,
        }
    }

    /// 单字节许可判定。
    pub const fn allows(self, b: u8) -> bool {
        let digit = b >= b'0' && b <= b'9';
        let lower = b >= b'a' && b <= b'z';
        let upper = b >= b'A' && b <= b'Z';
        match self {
            Charset::PathSafe => {
                digit || lower || upper || b == b'/' || b == b'.' || b == b'_' || b == b'-'
            }
            Charset::TagSafe => digit || lower || upper || b == b'-',
            Charset::OemSafe => digit || lower || upper || b == b'_',
            Charset::FacSafe => digit || lower || upper || b == b'.',
            Charset::ArgSafe => digit || lower || upper || b == b'.' || b == b'_' || b == b'-',
            Charset::HexSafe => digit || lower || upper,
        }
    }

    /// 整串许可判定（含控制字符与 `..` 序列双层闸——两闸同属 0x9E06 族）。
    pub fn admits(self, s: &str) -> bool {
        let bs = s.as_bytes();
        // 闸一：逐字节白名单（控制字符 0x00..=0x1F 与 0x7F 天然不在任何类内）。
        let mut i = 0;
        while i < bs.len() {
            if !self.allows(bs[i]) {
                return false;
            }
            i += 1;
        }
        // 闸二：`..` 序列禁（路径穿越形态提前截杀——不管类别）。
        let mut j = 0;
        while j + 1 < bs.len() {
            if bs[j] == b'.' && bs[j + 1] == b'.' {
                return false;
            }
            j += 1;
        }
        // 闸三：前导 `/` 禁（绝对路径注入面）。
        if !bs.is_empty() && bs[0] == b'/' {
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// §5 键描述（五元组：键名/类型/值域/默认值/语义注记）
// ---------------------------------------------------------------------------

/// 键规格（A1 认可集冻结稿逐键五元组）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    /// 键名（`unx.kernel.` 前缀——来源声明见册头要点一）。
    pub name: &'static str,
    /// 三型之一。
    pub ty: ParamType,
    /// 整数域下界（仅 Int 键有效；Bool/Str 键恒 0）。
    pub min: u64,
    /// 整数域上界（仅 Int 键有效；Bool/Str 键恒 0）。
    pub max: u64,
    /// 字符串长度上限（仅 Str 键有效；下界恒 1 非空）。
    pub str_max: usize,
    /// 字符白名单类别（仅 Str 键有效）。
    pub charset: Charset,
    /// 默认值（出厂值——查询回退落点）。
    pub default: ParamValue,
    /// 语义注记（A1 认可集冻结稿原文摘录——可审计）。
    pub note: &'static str,
}

impl KeySpec {
    /// 布尔键构造器。
    pub fn bool_key(name: &'static str, default: bool, note: &'static str) -> Self {
        KeySpec {
            name,
            ty: ParamType::Bool,
            min: 0,
            max: 0,
            str_max: 0,
            charset: Charset::HexSafe,
            default: ParamValue::B(default),
            note,
        }
    }

    /// 整数键构造器。
    pub fn int_key(
        name: &'static str,
        min: u64,
        max: u64,
        default: u64,
        note: &'static str,
    ) -> Self {
        KeySpec {
            name,
            ty: ParamType::Int,
            min,
            max,
            str_max: 0,
            charset: Charset::HexSafe,
            default: ParamValue::I(default),
            note,
        }
    }

    /// 字符串键构造器。
    pub fn str_key(
        name: &'static str,
        str_max: usize,
        charset: Charset,
        default: &'static str,
        note: &'static str,
    ) -> Self {
        KeySpec {
            name,
            ty: ParamType::Str,
            min: 0,
            max: 0,
            str_max,
            charset,
            default: ParamValue::S(default.to_string()),
            note,
        }
    }

    /// 指纹单键卷积（键名+类型+值域+字符集+默认值逐域入哈希）。
    pub fn fingerprint_contribute(&self, acc: u64) -> u64 {
        let a = fnv1a_bytes(acc, self.name.as_bytes());
        let a = fnv1a_byte(a, self.ty.disc());
        let a = fnv1a_u64(a, self.min);
        let a = fnv1a_u64(a, self.max);
        let a = fnv1a_u64(a, self.str_max as u64);
        let a = fnv1a_byte(a, self.charset.disc());
        match &self.default {
            ParamValue::B(b) => fnv1a_byte(a, if *b { 1 } else { 0 }),
            ParamValue::I(v) => fnv1a_u64(a, *v),
            ParamValue::S(s) => fnv1a_bytes(a, s.as_bytes()),
        }
    }
}

// ---------------------------------------------------------------------------
// §6 A1 认可集 24 键冻结表（布尔 8 / 整数 10 / 字符串 6）
// ---------------------------------------------------------------------------

/// 冻结稿版本号（语义来源声明——键名出处唯一凭据）。
pub const WLD_SPEC_VERSION: &str = "UNX-F0583/WLD-24 v1";
/// 键总数（判据冻结值）。
pub const WL_KEY_TOTAL: usize = 24;
/// 布尔键数。
pub const WL_BOOL_COUNT: usize = 8;
/// 整数键数。
pub const WL_INT_COUNT: usize = 10;
/// 字符串键数。
pub const WL_STR_COUNT: usize = 6;

/// 键槽位常量（0..=23——顺序即冻结序，指纹依赖此序）。
pub mod slot {
    /// 布尔键槽 0..=7。
    pub const B_SMP_ENABLED: usize = 0;
    pub const B_SERIAL_IRQ_SHARE: usize = 1;
    pub const B_ACPI_RESET_OVERRIDE: usize = 2;
    pub const B_MM_NX_ENFORCE: usize = 3;
    pub const B_TIME_HPET_PREFER: usize = 4;
    pub const B_FS_RO_BOOT: usize = 5;
    pub const B_DBG_KLOG_COLOR: usize = 6;
    pub const B_SEC_STRICT_TSC: usize = 7;
    /// 整数键槽 8..=17。
    pub const I_SERIAL_BAUD: usize = 8;
    pub const I_SMP_MAX_CPUS: usize = 9;
    pub const I_MM_HEAP_MIB: usize = 10;
    pub const I_TIMER_HZ: usize = 11;
    pub const I_IO_APIC_PINS: usize = 12;
    pub const I_FB_SCANLINES_MAX: usize = 13;
    pub const I_LOG_RING_LINES: usize = 14;
    pub const I_SEC_KASLR_BITS: usize = 15;
    pub const I_TIME_TSC_MARGIN_NS: usize = 16;
    pub const I_FS_INODE_CACHE_KB: usize = 17;
    /// 字符串键槽 18..=23。
    pub const S_BOOT_ROOT: usize = 18;
    pub const S_SERIAL_TAG: usize = 19;
    pub const S_ACPI_OEM_FILTER: usize = 20;
    pub const S_LOG_FACILITY: usize = 21;
    pub const S_BOOT_CMDLINE_KEY: usize = 22;
    pub const S_SEC_MACHINE_ID: usize = 23;
}

/// A1 认可集 24 键规格表（冻结稿逐键展开——顺序即槽位序）。
///
/// 来源声明：A1 认可集·白名单参数冻结草案 `UNX-F0583/WLD-24 v1`，
/// 键名统一 `unx.kernel.` 前缀，与常见同名（Linux `kernel.*`、
/// systemd、GRUB 等）无冲突。
pub fn build_spec_table() -> Vec<KeySpec> {
    let mut v: Vec<KeySpec> = vec![];
    // ---- 布尔型 8 键（槽 0..=7）----
    v.push(KeySpec::bool_key("unx.kernel.smp.enabled", true,
        "WLD-24/B0：SMP 子系统总开关；false 时 AP 全部滞留于 halt 环。"));
    v.push(KeySpec::bool_key("unx.kernel.serial.irq_share", false,
        "WLD-24/B1：串口中断是否允许共享槽位挂接；默认独占防串扰。"));
    v.push(KeySpec::bool_key("unx.kernel.acpi.reset_override", false,
        "WLD-24/B2：ACPI RESET_REG 失效时强制走键盘控制器复位回退。"));
    v.push(KeySpec::bool_key("unx.kernel.mm.nx_enforce", true,
        "WLD-24/B3：NX 位强制——数据页执行一律 PageFault 红账。"));
    v.push(KeySpec::bool_key("unx.kernel.time.hpet_prefer", true,
        "WLD-24/B4：校准源优先 HPET；false 时回退 PIT 双窗校准。"));
    v.push(KeySpec::bool_key("unx.kernel.fs.ro_boot", true,
        "WLD-24/B5：引导卷只读挂载；false 需 ADR 特批（冻结稿如此）。"));
    v.push(KeySpec::bool_key("unx.kernel.dbg.klog_color", false,
        "WLD-24/B6：内核日志 ANSI 着色；串口记录面默认素色。"));
    v.push(KeySpec::bool_key("unx.kernel.sec.strict_tsc", true,
        "WLD-24/B7：TSC 恒频假设严格模式；违规即 0x9C 族红账。"));
    // ---- 整数型 10 键（槽 8..=17）----
    v.push(KeySpec::int_key("unx.kernel.serial.baud",
        9600, 115200, 115200,
        "WLD-24/I0：串口波特率；域 9600..=115200，默认 115200。"));
    v.push(KeySpec::int_key("unx.kernel.smp.max_cpus",
        1, 64, 8,
        "WLD-24/I1：SMP 最大核数；域 1..=64，默认 8。"));
    v.push(KeySpec::int_key("unx.kernel.mm.heap_mib",
        4, 512, 64,
        "WLD-24/I2：内核堆预算 MiB；域 4..=512，默认 64。"));
    v.push(KeySpec::int_key("unx.kernel.timer.hz",
        10, 1000, 100,
        "WLD-24/I3：节拍频率 Hz；域 10..=1000，默认 100。"));
    v.push(KeySpec::int_key("unx.kernel.io.apic_pins",
        16, 240, 24,
        "WLD-24/I4：IOAPIC 可投递 pin 数；域 16..=240，默认 24。"));
    v.push(KeySpec::int_key("unx.kernel.fb.scanlines_max",
        240, 2160, 1080,
        "WLD-24/I5：帧缓冲扫描线预算上限；域 240..=2160，默认 1080。"));
    v.push(KeySpec::int_key("unx.kernel.log.ring_lines",
        64, 65536, 4096,
        "WLD-24/I6：内核日志环形账行数；域 64..=65536，默认 4096。"));
    v.push(KeySpec::int_key("unx.kernel.sec.kaslr_bits",
        4, 20, 12,
        "WLD-24/I7：KASLR 熵位宽；域 4..=20，默认 12。"));
    v.push(KeySpec::int_key("unx.kernel.time.tsc_margin_ns",
        0, 100000, 500,
        "WLD-24/I8：TSC deadline 写入安全余量 ns；域 0..=100000，默认 500。"));
    v.push(KeySpec::int_key("unx.kernel.fs.inode_cache_kb",
        16, 8192, 1024,
        "WLD-24/I9：inode 缓存预算 KiB；域 16..=8192，默认 1024。"));
    // ---- 字符串型 6 键（槽 18..=23）----
    v.push(KeySpec::str_key("unx.kernel.boot.root", 64, Charset::PathSafe,
        "boot/unx",
        "WLD-24/S0：引导根相对路径；path-safe 白名单，禁 .. 序列与前导 /。"));
    v.push(KeySpec::str_key("unx.kernel.serial.tag", 16, Charset::TagSafe,
        "ttyS0",
        "WLD-24/S1：串口设备标签；tag-safe 白名单。"));
    v.push(KeySpec::str_key("unx.kernel.acpi.oem_filter", 12, Charset::OemSafe,
        "ALL",
        "WLD-24/S2：ACPI OEM ID 过滤标签；oem-safe 白名单，ALL 即不过滤。"));
    v.push(KeySpec::str_key("unx.kernel.log.facility", 24, Charset::FacSafe,
        "klog.core",
        "WLD-24/S3：默认日志设施名；fac-safe 白名单（字母数字点）。"));
    v.push(KeySpec::str_key("unx.kernel.boot.cmdline_key", 32, Charset::ArgSafe,
        "unx.args",
        "WLD-24/S4：启动器参数键名；arg-safe 白名单。"));
    v.push(KeySpec::str_key("unx.kernel.sec.machine_id", 32, Charset::HexSafe,
        "0000000000000000",
        "WLD-24/S5：机器标识（hex 标签）；hex-safe 白名单全字母数字。"));
    v
}

// ---------------------------------------------------------------------------
// §7 三型校验器（严格集/钳外拒/长度+字符白名单/类型不符显性计数）
// ---------------------------------------------------------------------------

/// 手写 u64 十进制解析（严格：全数字、非空、溢出拒——返回 None）。
pub fn parse_u64_strict(s: &str) -> Option<u64> {
    let bs = s.as_bytes();
    if bs.is_empty() {
        return None;
    }
    let mut acc: u64 = 0;
    for &b in bs {
        if b < b'0' || b > b'9' {
            return None;
        }
        let d = (b - b'0') as u64;
        acc = acc.checked_mul(10)?.checked_add(d)?;
    }
    Some(acc)
}

/// 布尔严格集解析：true/false/0/1/yes/no 六 token（大小写敏感，不归一）。
pub fn parse_bool_strict(s: &str) -> Option<bool> {
    match s {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// 布尔严格集 token 是否合法（与 parse_bool_strict 同集——双入口同闸）。
pub fn bool_token_legal(s: &str) -> bool {
    parse_bool_strict(s).is_some()
}

/// 校验喂入值是否符合键规格。
///
/// 规则：① 类型不符 → TYPE_MISMATCH（显性计数由调用层落）；② 布尔值
/// 直接过（token 面校验由 `validate_bool_text` 承担）；③ 整数钳外拒；
/// ④ 字符串长度下界 1、上界 str_max、字符白名单六类。
pub fn validate_value(spec: &KeySpec, v: &ParamValue) -> Result<(), WlCode> {
    // 闸一：三型判别（类型不符显性——不静默转换）。
    if v.ty() != spec.ty {
        return Err(WlCode::TYPE_MISMATCH);
    }
    match v {
        ParamValue::B(_) => Ok(()),
        ParamValue::I(n) => {
            if *n < spec.min || *n > spec.max {
                Err(WlCode::INT_RANGE_OUT)
            } else {
                Ok(())
            }
        }
        ParamValue::S(s) => {
            let len = s.as_bytes().len();
            if len < 1 {
                return Err(WlCode::STR_TOO_LONG);
            }
            if len > spec.str_max {
                return Err(WlCode::STR_TOO_LONG);
            }
            if !spec.charset.admits(s) {
                return Err(WlCode::STR_CHAR_ILLEGAL);
            }
            Ok(())
        }
    }
}

/// 布尔键文本喂入校验（严格集——token 面闸，供注入面用）。
pub fn validate_bool_text(text: &str) -> Result<bool, WlCode> {
    match parse_bool_strict(text) {
        Some(b) => Ok(b),
        None => Err(WlCode::BOOL_PARSE_BAD),
    }
}

/// 整数键文本喂入校验（解析 + 钳——供注入面用）。
pub fn validate_int_text(spec: &KeySpec, text: &str) -> Result<u64, WlCode> {
    if spec.ty != ParamType::Int {
        return Err(WlCode::TYPE_MISMATCH);
    }
    match parse_u64_strict(text) {
        Some(n) => {
            if n < spec.min || n > spec.max {
                Err(WlCode::INT_RANGE_OUT)
            } else {
                Ok(n)
            }
        }
        None => Err(WlCode::TYPE_MISMATCH),
    }
}

/// 字符串键文本喂入校验（长度 + 白名单——供注入面用）。
pub fn validate_str_text(spec: &KeySpec, text: &str) -> Result<(), WlCode> {
    if spec.ty != ParamType::Str {
        return Err(WlCode::TYPE_MISMATCH);
    }
    let len = text.as_bytes().len();
    if len < 1 || len > spec.str_max {
        return Err(WlCode::STR_TOO_LONG);
    }
    if !spec.charset.admits(text) {
        return Err(WlCode::STR_CHAR_ILLEGAL);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §8 拒收统计账（类型不符/越域/字符非法/布尔坏/未知键/冻结拒——显性计数）
// ---------------------------------------------------------------------------

/// 拒收统计账（六分桶 + 总账——判据"白名单外拒收率 100%"的账面底座）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RejectStats {
    /// 白名单外键拒收数。
    pub unknown_key: u64,
    /// 类型不符拒收数。
    pub type_mismatch: u64,
    /// 布尔解析失败数。
    pub bool_parse_bad: u64,
    /// 整数越域数。
    pub int_range_out: u64,
    /// 字符串超长数。
    pub str_too_long: u64,
    /// 字符串字符非法数。
    pub str_char_illegal: u64,
    /// 冻结态写入全拒数。
    pub frozen_reject: u64,
    /// 喂入总数。
    pub total_in: u64,
    /// 拒收总数。
    pub total_reject: u64,
}

impl RejectStats {
    /// 单桶计数 + 总账双记账（零静默——每拒必落两笔）。
    pub fn count_reject(&mut self, bucket: Bucket) {
        match bucket {
            Bucket::UnknownKey => self.unknown_key += 1,
            Bucket::TypeMismatch => self.type_mismatch += 1,
            Bucket::BoolParseBad => self.bool_parse_bad += 1,
            Bucket::IntRangeOut => self.int_range_out += 1,
            Bucket::StrTooLong => self.str_too_long += 1,
            Bucket::StrCharIllegal => self.str_char_illegal += 1,
            Bucket::FrozenReject => self.frozen_reject += 1,
        }
        self.total_reject += 1;
    }

    /// 喂入计数（合法与拒收皆计——分母面）。
    pub fn count_in(&mut self) {
        self.total_in += 1;
    }

    /// 拒收率千分位：`reject*1000/injected`。
    ///
    /// 规则：① 分母零 → Err（无喂入不构成率）；② 分母非零 → 整数
    /// 千分位（全整数运算，零浮点）。
    pub fn reject_permille(&self) -> Result<u64, WlCode> {
        if self.total_in == 0 {
            return Err(WlCode::RATE_BELOW_THOUSAND);
        }
        Ok(self.total_reject * 1000 / self.total_in)
    }

    /// 六分桶 + 总账七账齐平校验（账行数与操作数失平 → LEDGER_GAP）。
    pub fn buckets_consistent(&self) -> bool {
        let sum = self.unknown_key
            + self.type_mismatch
            + self.bool_parse_bad
            + self.int_range_out
            + self.str_too_long
            + self.str_char_illegal
            + self.frozen_reject;
        sum == self.total_reject && self.total_reject <= self.total_in
    }
}

/// 拒收分桶闭集（统计账七桶）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// 白名单外键。
    UnknownKey,
    /// 类型不符。
    TypeMismatch,
    /// 布尔解析失败。
    BoolParseBad,
    /// 整数越域。
    IntRangeOut,
    /// 字符串超长。
    StrTooLong,
    /// 字符串字符非法。
    StrCharIllegal,
    /// 冻结态写入全拒。
    FrozenReject,
}

/// WlCode → 分桶映射（单点映射——校验器与统计账解耦但不漂移）。
pub const fn bucket_of(code: WlCode) -> Option<Bucket> {
    match code {
        WlCode::KEY_UNKNOWN => Some(Bucket::UnknownKey),
        WlCode::TYPE_MISMATCH => Some(Bucket::TypeMismatch),
        WlCode::BOOL_PARSE_BAD => Some(Bucket::BoolParseBad),
        WlCode::INT_RANGE_OUT => Some(Bucket::IntRangeOut),
        WlCode::STR_TOO_LONG => Some(Bucket::StrTooLong),
        WlCode::STR_CHAR_ILLEGAL => Some(Bucket::StrCharIllegal),
        WlCode::TABLE_FROZEN => Some(Bucket::FrozenReject),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// §9 表生命周期状态机（Draft → Frozen）与变更账 / ADR 留痕
// ---------------------------------------------------------------------------

/// 表生命周期两态状态机。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreezeState {
    /// 草拟态：set/insert/remove 合法（逐笔入变更账）。
    Draft,
    /// 冻结态：指纹落账，一切写入全拒 + ADR 立案。
    Frozen {
        /// 冻结时刻逻辑 tick。
        at_tick: u64,
        /// 冻结指纹（全表 FNV1a-64）。
        fingerprint: u64,
    },
}

impl FreezeState {
    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        matches!(self, FreezeState::Frozen { .. })
    }
}

/// 变更账行（冻结前合法变更逐笔留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRecord {
    /// 账序号（从 1 单调递增）。
    pub seq: u64,
    /// 变更逻辑 tick。
    pub tick: u64,
    /// 键名。
    pub key: String,
    /// 变更前值文本（None = insert）。
    pub before: Option<String>,
    /// 变更后值文本（None = remove）。
    pub after: Option<String>,
}

/// ADR 拒绝留痕行（冻结后写入尝试立案——架构决策记录流程底账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdrEntry {
    /// ADR 序号（从 1 单调递增）。
    pub seq: u64,
    /// 立案逻辑 tick。
    pub tick: u64,
    /// 被拒操作（set/insert/remove）。
    pub op: &'static str,
    /// 被拒键名。
    pub key: String,
    /// 被拒诊断码。
    pub code: WlCode,
    /// 立案说明（冻结语义原文摘录）。
    pub why: String,
}

// ---------------------------------------------------------------------------
// §10 白名单参数表主体
// ---------------------------------------------------------------------------

/// 白名单参数表（A1 认可集 24 键主体——规格 + 生效值 + 状态机 + 三账）。
#[derive(Debug, Clone)]
pub struct WlTable {
    /// 24 键规格（槽位序冻结）。
    specs: Vec<KeySpec>,
    /// 24 槽生效值（与 specs 同长同序）。
    values: Vec<ParamValue>,
    /// 生命周期状态。
    state: FreezeState,
    /// 变更账（冻结前合法变更）。
    ledger: Vec<ChangeRecord>,
    /// ADR 留痕账（冻结后拒绝立案）。
    adr: Vec<AdrEntry>,
    /// 拒收统计账。
    stats: RejectStats,
    /// 表构建逻辑 tick。
    built_tick: u64,
}

impl WlTable {
    /// 构建表（出厂态：24 键全默认值，Draft 态，三账清零）。
    ///
    /// 前置校验：① 恰 24 键；② 分布 8/10/6；③ 键名无重复。
    /// 任一不满足 → FREEZE_INCOMPLETE。
    pub fn build(tick: u64) -> Result<Self, WlErr> {
        let specs = build_spec_table();
        if specs.len() != WL_KEY_TOTAL {
            return Err(WlErr::new(WlCode::FREEZE_INCOMPLETE, tick, format!("规格表 {} 键 ≠ 冻结值 24", specs.len()).as_str()));
        }
        let mut n_bool: u64 = 0;
        let mut n_int: u64 = 0;
        let mut n_str: u64 = 0;
        for s in &specs {
            match s.ty {
                ParamType::Bool => n_bool += 1,
                ParamType::Int => n_int += 1,
                ParamType::Str => n_str += 1,
            }
        }
        if n_bool != WL_BOOL_COUNT as u64 || n_int != WL_INT_COUNT as u64 || n_str != WL_STR_COUNT as u64 {
            return Err(WlErr::new(WlCode::FREEZE_INCOMPLETE, tick, format!( "三型分布非 8/10/6（bool={} int={} str={}）", n_bool, n_int, n_str ) .as_str()));
        }
        // 键名查重（O(n²) 24 键常量面——清晰优先）。
        let mut i = 0;
        while i < specs.len() {
            let mut j = i + 1;
            while j < specs.len() {
                if specs[i].name == specs[j].name {
                    return Err(WlErr::new(WlCode::FREEZE_INCOMPLETE, tick, format!("键名重复：{}（槽 {} vs {}）", specs[i].name, i, j).as_str()));
                }
                j += 1;
            }
            i += 1;
        }
        let values: Vec<ParamValue> = specs.iter().map(|s| s.default.clone()).collect();
        Ok(WlTable {
            specs,
            values,
            state: FreezeState::Draft,
            ledger: Vec::new(),
            adr: Vec::new(),
            stats: RejectStats::default(),
            built_tick: tick,
        })
    }

    /// 全表指纹（逐键卷积——槽位序入哈希故依赖冻结序）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc = fnv1a_bytes(FNV1A_OFFSET, WLD_SPEC_VERSION.as_bytes());
        for s in &self.specs {
            acc = s.fingerprint_contribute(acc);
        }
        acc
    }

    /// 规格只读访问。
    pub fn specs(&self) -> &[KeySpec] {
        &self.specs
    }

    /// 生效值只读访问。
    pub fn values(&self) -> &[ParamValue] {
        &self.values
    }

    /// 生命周期状态只读访问。
    pub fn state(&self) -> &FreezeState {
        &self.state
    }

    /// 拒收统计账只读访问。
    pub fn stats(&self) -> &RejectStats {
        &self.stats
    }

    /// 变更账只读访问。
    pub fn ledger(&self) -> &[ChangeRecord] {
        &self.ledger
    }

    /// ADR 留痕账只读访问。
    pub fn adr(&self) -> &[AdrEntry] {
        &self.adr
    }

    /// 构建 tick。
    pub const fn built_tick(&self) -> u64 {
        self.built_tick
    }

    /// 键名 → 槽位（白名单成员判定——唯一权威入口）。
    pub fn find_slot(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.specs.len() {
            if self.specs[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 键存在性查询。
    pub fn contains(&self, name: &str) -> bool {
        self.find_slot(name).is_some()
    }

    /// 写入生效值（冻结语义执行点）。
    ///
    /// 规则：① 白名单外键 → KEY_UNKNOWN（不入 ADR——键不存在无从立案
    /// 键面）；② 冻结态 → TABLE_FROZEN 全拒 + ADR 立案；③ 值校验不过
    /// → 对应码（分桶计数）；④ 过 → 落值 + 变更账。
    pub fn set(&mut self, name: &str, v: ParamValue, tick: u64) -> Result<(), WlErr> {
        self.stats.count_in();
        let slot = match self.find_slot(name) {
            Some(s) => s,
            None => {
                self.stats.count_reject(Bucket::UnknownKey);
                return Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("键 {} 不在 A1 认可集 24 键内", name).as_str()));
            }
        };
        if self.state.is_frozen() {
            self.stats.count_reject(Bucket::FrozenReject);
            self.adr_push("set", name, WlCode::TABLE_FROZEN, tick);
            return Err(WlErr::new(WlCode::TABLE_FROZEN, tick, format!("键 {} 冻结后写入被拒——须走 ADR 流程", name).as_str()));
        }
        if let Err(code) = validate_value(&self.specs[slot], &v) {
            if let Some(b) = bucket_of(code) {
                self.stats.count_reject(b);
            }
            return Err(WlErr::new(code, tick, format!("键 {} 值校验拒：{}", name, v.to_kv_text()).as_str()));
        }
        let before = self.values[slot].to_kv_text();
        let after = v.to_kv_text();
        self.values[slot] = v;
        let seq = (self.ledger.len() as u64) + 1;
        self.ledger.push(ChangeRecord {
            seq,
            tick,
            key: name.to_string(),
            before: Some(before),
            after: Some(after),
        });
        Ok(())
    }

    /// 新键插入（冻结语义：任何时刻皆拒——白名单表闭集无扩容语义；
    /// Draft 态拒 KEY_UNKNOWN 语义上等价于"表结构只读"，走本入口
    /// 统一拒绝并计数）。
    pub fn insert(&mut self, name: &str, _v: ParamValue, tick: u64) -> Result<(), WlErr> {
        self.stats.count_in();
        self.stats.count_reject(Bucket::UnknownKey);
        if self.state.is_frozen() {
            self.adr_push("insert", name, WlCode::TABLE_FROZEN, tick);
        }
        Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("键 {} 插入被拒：白名单表为闭集，无扩容语义", name).as_str()))
    }

    /// 键删除（冻结语义同 insert：闭集无删减语义——任何时刻皆拒）。
    pub fn remove(&mut self, name: &str, tick: u64) -> Result<(), WlErr> {
        self.stats.count_in();
        self.stats.count_reject(Bucket::UnknownKey);
        if self.state.is_frozen() {
            self.adr_push("remove", name, WlCode::TABLE_FROZEN, tick);
        }
        Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("键 {} 删除被拒：白名单表为闭集，无删减语义", name).as_str()))
    }

    /// ADR 立案（序号单调）。
    fn adr_push(&mut self, op: &'static str, key: &str, code: WlCode, tick: u64) {
        let seq = (self.adr.len() as u64) + 1;
        self.adr.push(AdrEntry {
            seq,
            tick,
            op,
            key: key.to_string(),
            code,
            why: "冻结后写入尝试全拒——架构决策须经 ADR 流程留痕后方可评估改冻结稿"
                .to_string(),
        });
    }

    /// 冻结（Draft → Frozen）。
    ///
    /// 前置：① Draft 态（已冻结 → TABLE_FROZEN）；② 恰 24 键分布 8/10/6
    /// 且无重名（FREEZE_INCOMPLETE）。通过 → 指纹落账入状态机。
    pub fn freeze(&mut self, tick: u64) -> Result<u64, WlErr> {
        if self.state.is_frozen() {
            return Err(WlErr::new(WlCode::TABLE_FROZEN, tick, "表已在冻结态——二次冻结拒"));
        }
        if self.specs.len() != WL_KEY_TOTAL {
            return Err(WlErr::new(WlCode::FREEZE_INCOMPLETE, tick, format!("冻结前置：{} 键 ≠ 24", self.specs.len()).as_str()));
        }
        let mut n_bool: u64 = 0;
        let mut n_int: u64 = 0;
        let mut n_str: u64 = 0;
        for s in &self.specs {
            match s.ty {
                ParamType::Bool => n_bool += 1,
                ParamType::Int => n_int += 1,
                ParamType::Str => n_str += 1,
            }
        }
        if n_bool != WL_BOOL_COUNT as u64 || n_int != WL_INT_COUNT as u64 || n_str != WL_STR_COUNT as u64 {
            return Err(WlErr::new(WlCode::FREEZE_INCOMPLETE, tick, "冻结前置：三型分布非 8/10/6"));
        }
        let fp = self.fingerprint();
        self.state = FreezeState::Frozen {
            at_tick: tick,
            fingerprint: fp,
        };
        Ok(fp)
    }

    /// 冻结指纹读取（未冻结 → None）。
    pub fn frozen_fingerprint(&self) -> Option<u64> {
        match &self.state {
            FreezeState::Frozen { fingerprint, .. } => Some(*fingerprint),
            FreezeState::Draft => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §11 查询 API（存在性 / 类型安全取值 / 默认值回退带账注）
// ---------------------------------------------------------------------------

/// 查询命中账注（回退语义显性——取值来源可审计）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHit {
    /// 键名。
    pub key: String,
    /// 命中值。
    pub value: ParamValue,
    /// 是否为默认值回退（true = 生效值缺位/未定制，取的出厂值）。
    pub from_default: bool,
    /// 查询逻辑 tick。
    pub tick: u64,
}

/// 查询三件套包装（存在性 + 类型安全 + 回退账注）。
pub struct QueryApi<'a> {
    /// 表引用。
    pub table: &'a WlTable,
}

impl<'a> QueryApi<'a> {
    /// 新建查询面。
    pub const fn new(table: &'a WlTable) -> Self {
        QueryApi { table }
    }

    /// 存在性（同 contains——语义别名，账面阅读友好）。
    pub fn exists(&self, name: &str) -> bool {
        self.table.contains(name)
    }

    /// 布尔类型安全取值（类型不符 → TYPE_MISMATCH；不存在 → KEY_UNKNOWN）。
    pub fn get_bool(&self, name: &str, tick: u64) -> Result<bool, WlErr> {
        let slot = match self.table.find_slot(name) {
            Some(s) => s,
            None => {
                return Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("查询键 {} 不在白名单", name).as_str()))
            }
        };
        match &self.table.values[slot] {
            ParamValue::B(b) => Ok(*b),
            other => Err(WlErr::new(WlCode::TYPE_MISMATCH, tick, format!("键 {} 生效值非布尔型（实为 {}）", name, other.ty().name()).as_str())),
        }
    }

    /// 整数类型安全取值。
    pub fn get_int(&self, name: &str, tick: u64) -> Result<u64, WlErr> {
        let slot = match self.table.find_slot(name) {
            Some(s) => s,
            None => {
                return Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("查询键 {} 不在白名单", name).as_str()))
            }
        };
        match &self.table.values[slot] {
            ParamValue::I(n) => Ok(*n),
            other => Err(WlErr::new(WlCode::TYPE_MISMATCH, tick, format!("键 {} 生效值非整数型（实为 {}）", name, other.ty().name()).as_str())),
        }
    }

    /// 字符串类型安全取值。
    pub fn get_str(&self, name: &str, tick: u64) -> Result<String, WlErr> {
        let slot = match self.table.find_slot(name) {
            Some(s) => s,
            None => {
                return Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("查询键 {} 不在白名单", name).as_str()))
            }
        };
        match &self.table.values[slot] {
            ParamValue::S(s) => Ok(s.clone()),
            other => Err(WlErr::new(WlCode::TYPE_MISMATCH, tick, format!("键 {} 生效值非字符串型（实为 {}）", name, other.ty().name()).as_str())),
        }
    }

    /// 默认值回退取值（带账注）。
    ///
    /// 规则：① 键不存在 → KEY_UNKNOWN（白名单外无回退语义——回退
    /// 只发生在认可集内）；② 类型不符 → TYPE_MISMATCH（调用方要的
    /// 型与键声明不一致——不静默转换）；③ 其余 → 生效值或出厂默认，
    /// QueryHit::from_default 显性标注（生效值 == 默认值时也标 true——
    /// 回退账注看的是值相等不是写入历史，确定性优先）。
    pub fn get_or_default(&self, name: &str, want: ParamType, tick: u64) -> Result<QueryHit, WlErr> {
        let slot = match self.table.find_slot(name) {
            Some(s) => s,
            None => {
                return Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("回退查询键 {} 不在白名单——无回退语义", name).as_str()))
            }
        };
        let spec = &self.table.specs[slot];
        if spec.ty != want {
            return Err(WlErr::new(WlCode::TYPE_MISMATCH, tick, format!( "回退查询键 {} 声明 {} 而调用方要 {}", name, spec.ty.name(), want.name() ) .as_str()));
        }
        let cur = &self.table.values[slot];
        let is_default = *cur == spec.default;
        Ok(QueryHit {
            key: name.to_string(),
            value: cur.clone(),
            from_default: is_default,
            tick,
        })
    }
}

// ---------------------------------------------------------------------------
// §12 注入攻击面六样本闭集（6/6 全拒对拍账）
// ---------------------------------------------------------------------------

/// 注入样本（六闭集成员——id 1..=6 固定序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectSample {
    /// 样本号（1..=6）。
    pub id: u64,
    /// 样本名（人话）。
    pub name: &'static str,
    /// 目标键名。
    pub key: &'static str,
    /// 喂入文本（注入载荷）。
    pub payload: &'static str,
    /// 预期拒收码。
    pub expect: WlCode,
}

/// 六样本闭集（判据冻结序：超范围整数/非法布尔/超长字符串/路径穿越串/
/// 控制字符注入/类型混淆）。
pub fn inject_samples() -> Vec<InjectSample> {
    let mut v: Vec<InjectSample> = vec![];
    v.push(InjectSample { id: 1, name: "超范围整数",
        key: "unx.kernel.serial.baud", payload: "4000000",
        expect: WlCode::INT_RANGE_OUT });
    v.push(InjectSample { id: 2, name: "非法布尔",
        key: "unx.kernel.smp.enabled", payload: "maybe",
        expect: WlCode::BOOL_PARSE_BAD });
    v.push(InjectSample { id: 3, name: "超长字符串",
        key: "unx.kernel.boot.root",
        payload: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        expect: WlCode::STR_TOO_LONG });
    v.push(InjectSample { id: 4, name: "路径穿越串",
        key: "unx.kernel.boot.root", payload: "../etc/passwd",
        expect: WlCode::STR_CHAR_ILLEGAL });
    v.push(InjectSample { id: 5, name: "控制字符注入",
        key: "unx.kernel.serial.tag", payload: "tty\x1b[31mS0",
        expect: WlCode::STR_CHAR_ILLEGAL });
    v.push(InjectSample { id: 6, name: "类型混淆",
        key: "unx.kernel.smp.max_cpus", payload: "not-a-number",
        expect: WlCode::TYPE_MISMATCH });
    v
}

/// 注入对拍账行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectRecord {
    /// 样本号。
    pub id: u64,
    /// 样本名。
    pub name: String,
    /// 实际拒收码。
    pub got: WlCode,
    /// 预期拒收码是否命中。
    pub matched: bool,
    /// 立案 tick。
    pub tick: u64,
}

/// 注入对拍总账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectReport {
    /// 逐样本对拍行（恰 6 行）。
    pub records: Vec<InjectRecord>,
    /// 注入总数（6）。
    pub injected: u64,
    /// 拒收总数（判据要求 6）。
    pub rejected: u64,
    /// 拒收率千分位（判据要求恰 1000）。
    pub permille: u64,
    /// 6/6 达标判定。
    pub all_rejected: bool,
}

/// 注入六样本闭集执行（表必须冻结态——冻结语义下的注入对拍最严）。
///
/// 规则：① 样本按文本载荷路径喂（与真实启动参数同路径）；② 每个
/// 样本期望 Err 且码匹配 expect——拒码不符也算对拍红；③ 全拒 →
/// permille = rejected*1000/injected 恰 1000。
pub fn run_inject_suite(table: &mut WlTable, tick: u64) -> Result<InjectReport, WlErr> {
    let samples = inject_samples();
    let mut records: Vec<InjectRecord> = Vec::new();
    let mut rejected: u64 = 0;
    let base_in = table.stats().total_in;
    let base_rej = table.stats().total_reject;
    for s in &samples {
        // 样本统一走 set 文本入口（与启动参数装载同路径）。
        let res = set_text(table, s.key, s.payload, tick);
        let (got, matched) = match res {
            Err(e) => (e.code, e.code == s.expect),
            Ok(()) => (WlCode::INJECT_REJECTED, false),
        };
        if matched {
            rejected += 1;
        }
        records.push(InjectRecord {
            id: s.id,
            name: s.name.to_string(),
            got,
            matched,
            tick,
        });
    }
    let injected = table.stats().total_in - base_in;
    let rejected_now = table.stats().total_reject - base_rej;
    if table.stats().total_in == 0 {
        return Err(WlErr::new(WlCode::RATE_BELOW_THOUSAND, tick, "注入对拍：分母为零（无喂入）"));
    }
    let permille = rejected_now * 1000 / injected;
    let all_rejected = rejected == 6 && injected == 6 && rejected_now == 6 && permille == 1000;
    if !all_rejected {
        // 未达标红账（判据兜底——RATE 码显性，不静默）。
        return Err(WlErr::new(WlCode::RATE_BELOW_THOUSAND, tick, format!( "注入对拍未达标：{}/{} 拒，permille={}", rejected_now, injected, permille ) .as_str()));
    }
    Ok(InjectReport {
        records,
        injected,
        rejected: rejected_now,
        permille,
        all_rejected,
    })
}

/// 文本入口写入（启动参数装载同路径：按键类型分派三型文本校验）。
///
/// 规则：① 白名单外 → KEY_UNKNOWN；② 冻结 → TABLE_FROZEN；③ 布尔键
/// 严格集解析（token 坏 → BOOL_PARSE_BAD）；④ 整数键手写解析 + 钳
/// （解析坏 → TYPE_MISMATCH，钳外 → INT_RANGE_OUT）；⑤ 字符串键长度
/// + 白名单；⑥ 过 → set 落值。
pub fn set_text(table: &mut WlTable, name: &str, text: &str, tick: u64) -> Result<(), WlErr> {
    let slot = match table.find_slot(name) {
        Some(s) => s,
        None => {
            table.stats.count_in();
            table.stats.count_reject(Bucket::UnknownKey);
            return Err(WlErr::new(WlCode::KEY_UNKNOWN, tick, format!("文本入口键 {} 不在白名单", name).as_str()));
        }
    };
    if table.state.is_frozen() {
        table.stats.count_in();
        table.stats.count_reject(Bucket::FrozenReject);
        table.adr_push("set_text", name, WlCode::TABLE_FROZEN, tick);
        return Err(WlErr::new(WlCode::TABLE_FROZEN, tick, format!("键 {} 冻结后文本写入被拒", name).as_str()));
    }
    let spec = table.specs[slot].clone();
    let value: ParamValue = match spec.ty {
        ParamType::Bool => match validate_bool_text(text) {
            Ok(b) => ParamValue::B(b),
            Err(code) => {
                table.stats.count_in();
                table.stats.count_reject(Bucket::BoolParseBad);
                return Err(WlErr::new(code, tick, format!("键 {} 布尔 token '{}' 不在严格集", name, text).as_str()));
            }
        },
        ParamType::Int => match validate_int_text(&spec, text) {
            Ok(n) => ParamValue::I(n),
            Err(code) => {
                table.stats.count_in();
                let b = bucket_of(code).unwrap_or(Bucket::IntRangeOut);
                table.stats.count_reject(b);
                return Err(WlErr::new(code, tick, format!("键 {} 整数文本 '{}' 校验拒", name, text).as_str()));
            }
        },
        ParamType::Str => match validate_str_text(&spec, text) {
            Ok(()) => ParamValue::S(text.to_string()),
            Err(code) => {
                table.stats.count_in();
                let b = bucket_of(code).unwrap_or(Bucket::StrCharIllegal);
                table.stats.count_reject(b);
                return Err(WlErr::new(code, tick, format!("键 {} 字符串文本 '{}' 校验拒", name, text).as_str()));
            }
        },
    };
    table.set(name, value, tick)
}

// ---------------------------------------------------------------------------
// §13 值域边界三值对拍（min/max 收、min-1/max+1 拒——逐键账）
// ---------------------------------------------------------------------------

/// 单键边界对拍行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryKey {
    /// 键名。
    pub key: String,
    /// min 收通过。
    pub min_ok: bool,
    /// max 收通过。
    pub max_ok: bool,
    /// min-1 拒通过。
    pub min_minus_ok: bool,
    /// max+1 拒通过。
    pub max_plus_ok: bool,
    /// 全通过。
    pub all_ok: bool,
}

/// 边界对拍总账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryReport {
    /// 逐键行（整数 10 键 + 布尔 8 键 + 字符串 6 键 = 24 行）。
    pub rows: Vec<BoundaryKey>,
    /// 全绿判定。
    pub all_ok: bool,
}

/// 值域边界三值对拍（对临时克隆表执行——不污染主表生效值）。
///
/// 规则：① 整数键 min/max 必收、min-1/max+1 必拒（下溢饱和语义：
/// min=0 时 min-1 用 saturating_sub——仍必拒）；② 布尔键 true/false
/// 必收、严格集外 token（"on"/"off"）必拒；③ 字符串键 max 长必收、
/// max+1 长必拒、零长必拒。任一不符 → BOUNDARY_MISMATCH。
pub fn run_boundary_suite(table: &WlTable, tick: u64) -> Result<BoundaryReport, WlErr> {
    let mut rows: Vec<BoundaryKey> = Vec::new();
    let mut all_ok = true;
    for spec in table.specs() {
        let row = match spec.ty {
            ParamType::Int => {
                let min_in = validate_value(spec, &ParamValue::I(spec.min)).is_ok();
                let max_in = validate_value(spec, &ParamValue::I(spec.max)).is_ok();
                let min_minus = spec.min.saturating_sub(1);
                let min_out = validate_value(spec, &ParamValue::I(min_minus)).is_err();
                let max_plus = spec.max.saturating_add(1);
                // max+1 溢出饱和回 max 时（max=u64::MAX），跳过越界判——
                // 本表域内无此形态，仍按饱和值判拒（域内 max 可收则
                // 饱和值==max 会误红；故判"值大于 max 即拒"恒真面）。
                let max_out = if max_plus > spec.max {
                    validate_value(spec, &ParamValue::I(max_plus)).is_err()
                } else {
                    true
                };
                let ok = min_in && max_in && min_out && max_out;
                BoundaryKey {
                    key: spec.name.to_string(),
                    min_ok: min_in,
                    max_ok: max_in,
                    min_minus_ok: min_out,
                    max_plus_ok: max_out,
                    all_ok: ok,
                }
            }
            ParamType::Bool => {
                let t_in = validate_bool_text("true").is_ok();
                let f_in = validate_bool_text("false").is_ok();
                let one_in = validate_bool_text("1").is_ok() && validate_bool_text("0").is_ok();
                let yes_in = validate_bool_text("yes").is_ok() && validate_bool_text("no").is_ok();
                let on_out = validate_bool_text("on").is_err();
                let off_out = validate_bool_text("off").is_err();
                let upper_out = validate_bool_text("True").is_err();
                let ok = t_in && f_in && one_in && yes_in && on_out && off_out && upper_out;
                BoundaryKey {
                    key: spec.name.to_string(),
                    min_ok: t_in && f_in,
                    max_ok: one_in && yes_in,
                    min_minus_ok: on_out && off_out,
                    max_plus_ok: upper_out,
                    all_ok: ok,
                }
            }
            ParamType::Str => {
                // max 长必收（默认值同长重造——白名单字符 'a' 仅当默认
                // 字符集许可；六类白名单全含 a-z 故 'a' 恒许可）。
                let fill: String = {
                    let mut s = String::new();
                    let mut i = 0;
                    while i < spec.str_max {
                        s.push('a');
                        i += 1;
                    }
                    s
                };
                let max_in = validate_value(spec, &ParamValue::S(fill.clone())).is_ok();
                // max+1 长必拒。
                let mut over = fill.clone();
                over.push('a');
                let max_out = validate_value(spec, &ParamValue::S(over)).is_err();
                // 零长（min-1 = 0 长度）必拒。
                let zero_out = validate_value(spec, &ParamValue::S(String::new())).is_err();
                let ok = max_in && max_out && zero_out;
                BoundaryKey {
                    key: spec.name.to_string(),
                    min_ok: zero_out,
                    max_ok: max_in,
                    min_minus_ok: zero_out,
                    max_plus_ok: max_out,
                    all_ok: ok,
                }
            }
        };
        if !row.all_ok {
            all_ok = false;
        }
        rows.push(row);
    }
    if !all_ok {
        let mut bad = 0u64;
        for r in &rows {
            if !r.all_ok {
                bad += 1;
            }
        }
        return Err(WlErr::new(WlCode::BOUNDARY_MISMATCH, tick, format!("边界对拍 {} 键红", bad).as_str()));
    }
    Ok(BoundaryReport { rows, all_ok })
}

// ---------------------------------------------------------------------------
// §14 表导出（kv + TLV 双形态 + 指纹往返）
// ---------------------------------------------------------------------------

/// TLV 记录（tag = 槽位+1，1..=24 闭集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlvRecord {
    /// tag（槽位+1）。
    pub tag: u8,
    /// 载荷（键名\0 + ty + charset + min + max + strmax + value）。
    pub payload: Vec<u8>,
}

/// kv 导出形态（键=值文本对——槽位序）。
pub fn export_kv(table: &WlTable) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < table.specs().len() {
        out.push((
            table.specs()[i].name.to_string(),
            table.values()[i].to_kv_text(),
        ));
        i += 1;
    }
    out
}

/// 单键 TLV 载荷编码（键名\0 + ty(1) + charset(1) + min(8) + max(8) +
/// strmax(2) + value 文本）。
fn encode_payload(spec: &KeySpec, value: &ParamValue) -> Vec<u8> {
    let mut p: Vec<u8> = Vec::new();
    let nb = spec.name.as_bytes();
    let mut i = 0;
    while i < nb.len() {
        p.push(nb[i]);
        i += 1;
    }
    p.push(0);
    p.push(spec.ty.disc());
    p.push(spec.charset.disc());
    let min_b = spec.min.to_le_bytes();
    let max_b = spec.max.to_le_bytes();
    let sm_b = (spec.str_max as u64).to_le_bytes();
    let mut j = 0;
    while j < 8 {
        p.push(min_b[j]);
        j += 1;
    }
    j = 0;
    while j < 8 {
        p.push(max_b[j]);
        j += 1;
    }
    j = 0;
    while j < 2 {
        p.push(sm_b[j]);
        j += 1;
    }
    let vb = value.to_kv_text();
    let vb = vb.as_bytes();
    j = 0;
    while j < vb.len() {
        p.push(vb[j]);
        j += 1;
    }
    p
}

/// TLV 导出（tag/len/payload 三元流——字节面确定性）。
pub fn export_tlv(table: &WlTable) -> Result<Vec<u8>, WlErr> {
    let tick = table.built_tick();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < table.specs().len() {
        let tag = (i + 1) as u8;
        if tag == 0 {
            return Err(WlErr::new(WlCode::EXPORT_TLV_BAD, tick, "tag 溢出（槽位 > 255——表结构漂移）"));
        }
        let payload = encode_payload(&table.specs()[i], &table.values()[i]);
        let len = payload.len() as u16;
        out.push(tag);
        out.push((len & 0xFF) as u8);
        out.push((len >> 8) as u8);
        let mut k = 0;
        while k < payload.len() {
            out.push(payload[k]);
            k += 1;
        }
        i += 1;
    }
    Ok(out)
}

/// TLV 单记录回读解析。
pub fn parse_tlv_record(buf: &[u8], off: usize) -> Result<(TlvRecord, usize), WlCode> {
    if off + 3 > buf.len() {
        return Err(WlCode::EXPORT_TLV_BAD);
    }
    let tag = buf[off];
    let len = u16::from_le_bytes([buf[off + 1], buf[off + 2]]) as usize;
    let start = off + 3;
    if start + len > buf.len() {
        return Err(WlCode::EXPORT_TLV_BAD);
    }
    if tag == 0 || tag as usize > WL_KEY_TOTAL {
        return Err(WlCode::EXPORT_TLV_BAD);
    }
    let mut payload: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < len {
        payload.push(buf[start + i]);
        i += 1;
    }
    Ok((TlvRecord { tag, payload }, start + len))
}

/// TLV 载荷回读重建键规格指纹贡献（与 encode_payload 对偶——往返底座）。
///
/// 结构：name\0 ty charset min[8] max[8] strmax[2] value-text。
/// 载荷结构不完整/判别值非法 → EXPORT_TLV_BAD。
pub fn payload_fingerprint_contribute(payload: &[u8], acc: u64) -> Result<u64, WlCode> {
    // 键名段（\0 结尾）。
    let mut name_end: Option<usize> = None;
    let mut i = 0;
    while i < payload.len() {
        if payload[i] == 0 {
            name_end = Some(i);
            break;
        }
        i += 1;
    }
    let name_end = match name_end {
        Some(e) if e > 0 => e,
        _ => return Err(WlCode::EXPORT_TLV_BAD),
    };
    // 固定结构段：ty(1) + charset(1) + min(8) + max(8) + strmax(2) = 20。
    let fixed_at = name_end + 1;
    if payload.len() < fixed_at + 20 {
        return Err(WlCode::EXPORT_TLV_BAD);
    }
    let ty_d = payload[fixed_at];
    let cs_d = payload[fixed_at + 1];
    if ParamType::of_disc(ty_d).is_none() || Charset::of_disc(cs_d).is_none() {
        return Err(WlCode::EXPORT_TLV_BAD);
    }
    let mut min_b = [0u8; 8];
    let mut max_b = [0u8; 8];
    let mut sm_b = [0u8; 2];
    let mut j = 0;
    while j < 8 {
        min_b[j] = payload[fixed_at + 2 + j];
        max_b[j] = payload[fixed_at + 10 + j];
        j += 1;
    }
    j = 0;
    while j < 2 {
        sm_b[j] = payload[fixed_at + 18 + j];
        j += 1;
    }
    let min = u64::from_le_bytes(min_b);
    let max = u64::from_le_bytes(max_b);
    let sm = u16::from_le_bytes(sm_b) as u64;
    let a = fnv1a_bytes(acc, &payload[0..name_end]);
    let a = fnv1a_byte(a, ty_d);
    let a = fnv1a_u64(a, min);
    let a = fnv1a_u64(a, max);
    let a = fnv1a_u64(a, sm);
    let a = fnv1a_byte(a, cs_d);
    // 默认值域：value 文本（尾部剩余字节）。
    let val_start = fixed_at + 20;
    let mut vb: Vec<u8> = Vec::new();
    let mut k = val_start;
    while k < payload.len() {
        vb.push(payload[k]);
        k += 1;
    }
    Ok(fnv1a_bytes(a, &vb))
}

/// TLV 回读全表指纹重建（逐记录卷积——与 export 侧同槽位序）。
pub fn tlv_rebuild_fingerprint(buf: &[u8]) -> Result<u64, WlCode> {
    let mut acc = fnv1a_bytes(FNV1A_OFFSET, WLD_SPEC_VERSION.as_bytes());
    let mut off = 0usize;
    let mut count = 0usize;
    while off < buf.len() {
        let (rec, next) = parse_tlv_record(buf, off)?;
        acc = payload_fingerprint_contribute(&rec.payload, acc)?;
        count += 1;
        off = next;
    }
    if count != WL_KEY_TOTAL {
        return Err(WlCode::EXPORT_TLV_BAD);
    }
    Ok(acc)
}

/// 指纹往返对拍（TLV 回读重建指纹 == 表当前指纹——漂移即 FINGERPRINT_DRIFT）。
pub fn fingerprint_roundtrip(table: &WlTable, tick: u64) -> Result<(), WlErr> {
    let buf = export_tlv(table)?;
    let rebuilt = match tlv_rebuild_fingerprint(&buf) {
        Ok(fp) => fp,
        Err(code) => {
            return Err(WlErr::new(code, tick, "TLV 回读重建失败——结构面坏"))
        }
    };
    let cur = table.fingerprint();
    if rebuilt != cur {
        return Err(WlErr::new(WlCode::FINGERPRINT_DRIFT, tick, format!( "回读指纹 0x{:016X} ≠ 表指纹 0x{:016X}", rebuilt, cur ) .as_str()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §15 端到端总装配（判据闭环：冻结 → 注入 6/6 → 率 1000‰ → 往返）
// ---------------------------------------------------------------------------

/// 端到端总账（判据 J1 逐项落账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlFinalReport {
    /// 全表指纹。
    pub fingerprint: u64,
    /// 冻结 tick。
    pub frozen_tick: u64,
    /// 注入对拍账（6/6）。
    pub inject: InjectReport,
    /// 边界对拍键数（24）。
    pub boundary_rows: usize,
    /// TLV 往返通过。
    pub roundtrip_ok: bool,
    /// 拒收率千分位（恰 1000）。
    pub permille: u64,
    /// ADR 立案数（冻结后写入尝试）。
    pub adr_count: u64,
}

/// 端到端总装配（一次调用闭环判据 J1——全确定性，tick 驱动）。
///
/// 流程：① build（出厂默认 Draft）；② 边界三值对拍（Draft 态合法
/// 值面校验）；③ 冻结（指纹落账）；④ 冻结后写入尝试（ADR 立案账）；
/// ⑤ 注入六样本对拍（6/6 全拒）；⑥ 率千分位恰 1000 判定；⑦ TLV
/// 指纹往返。任一环节红 → 显性 Err。
pub fn assemble_final(tick: u64) -> Result<WlFinalReport, WlErr> {
    // ① 构建。
    let mut table = WlTable::build(tick)?;
    // ② 边界三值对拍（Draft 态——校验器纯函数面，不改生效值）。
    let br = run_boundary_suite(&table, tick)?;
    if br.rows.len() != WL_KEY_TOTAL {
        return Err(WlErr::new(WlCode::LEDGER_GAP, tick, format!("边界对拍 {} 行 ≠ 24", br.rows.len()).as_str()));
    }
    // ③ 冻结。
    let fp = table.freeze(tick)?;
    // ④ 冻结后写入尝试（合法键合法值——冻结语义全拒 + ADR 立案）。
    let _ = table.set("unx.kernel.serial.baud", ParamValue::I(9600), tick);
    let _ = table.set("unx.kernel.smp.enabled", ParamValue::B(true), tick);
    if table.adr().len() < 2 {
        return Err(WlErr::new(WlCode::LEDGER_GAP, tick, "冻结后写入尝试 ADR 立案不足 2 笔——冻结语义漏账"));
    }
    // ⑤ 注入六样本对拍（6/6 全拒）。
    let inject = run_inject_suite(&mut table, tick)?;
    if inject.records.len() != 6 || inject.rejected != 6 {
        return Err(WlErr::new(WlCode::LEDGER_GAP, tick, format!( "注入对拍账 {} 行 / {} 拒 ≠ 6/6", inject.records.len(), inject.rejected ) .as_str()));
    }
    // ⑥ 率千分位恰 1000 判定（run_inject_suite 已兜底，此处双确认）。
    let permille = match table.stats().reject_permille() {
        Ok(p) => p,
        Err(_) => {
            return Err(WlErr::new(WlCode::RATE_BELOW_THOUSAND, tick, "拒收率千分位计算失准（分母为零）"))
        }
    };
    if permille != 1000 {
        return Err(WlErr::new(WlCode::RATE_BELOW_THOUSAND, tick, format!("全表拒收率 {}‰ ≠ 1000‰", permille).as_str()));
    }
    // ⑦ TLV 指纹往返。
    fingerprint_roundtrip(&table, tick)?;
    Ok(WlFinalReport {
        fingerprint: fp,
        frozen_tick: tick,
        inject,
        boundary_rows: br.rows.len(),
        roundtrip_ok: true,
        permille,
        adr_count: table.adr().len() as u64,
    })
}

/// 统计账齐平终检（六分桶 + 总账 + ADR——黑匣子漏账显性）。
pub fn stats_final_audit(table: &WlTable, tick: u64) -> Result<(), WlErr> {
    if !table.stats().buckets_consistent() {
        return Err(WlErr::new(WlCode::LEDGER_GAP, tick, "拒收统计账分桶与总账失平"));
    }
    let seq_ok = {
        let mut ok = true;
        let mut expect = 1u64;
        for a in table.adr() {
            if a.seq != expect {
                ok = false;
            }
            expect += 1;
        }
        ok
    };
    if !seq_ok {
        return Err(WlErr::new(WlCode::LEDGER_GAP, tick, "ADR 立案序号断档"));
    }
    Ok(())
}
