//! UNX-F50072 纸张错误码族（15 条：缺纸/卡纸/尺寸不符等）（0x43520 段）
//!
//! 锚点：UNX-F50072-J1 错误码 ×15 零漂移；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x43520`，诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 10`，
//! 逐项见 [`PaperErrError`]。
//!
//! 为什么纸张错误要独立成「码族」而不是散落的 bool：走纸是唯一会**伤人**
//! （夹手 / 撕纸 / 卡机）且**必须给出可执行恢复动作**的子系统。15 条错误
//! 各自绑定「族 / 严重度 / 恢复动作 / 自动重试预算」四元组；这四个字段是
//! 现场手册、售后脚本与面板提示文案的共同事实源，任何一处漂移都会让三条
//! 链路同时错。故本模块把这张表钉成**冻结摘要**：只要有人改动任一条目的
//! 任一字段，`canonical_digest()` 立刻变化，J1「零漂移」当场当红。
//!
//! 结构清单：
//! * [`PaperFault`]：15 条错误码枚举（码值 1..=15 闭集，`from_code` 反解）；
//! * [`FaultSpec`] + [`FaultRegistry`]：规格注册表（去重 / 定容 / 封账 / 完整性）；
//! * [`FaultEvent`] + [`FaultLedger`] + [`FaultReport`]：故障事件账
//!   （逐码计数守恒 / 严重度上确界 / 顺序摘要）；
//! * [`injection_core`]：6 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43520 独占 0x20）。
pub const SEG_BASE: u32 = 0x43520;

const E_EMPTY: u16 = 1;
const E_UNKNOWN_CODE: u16 = 2;
const E_DUPLICATE: u16 = 3;
const E_REGISTRY_FULL: u16 = 4;
const E_POLICY: u16 = 5;
const E_LEDGER_FULL: u16 = 6;
const E_SEALED: u16 = 7;
const E_BAD_EVENT: u16 = 8;
const E_INCOMPLETE: u16 = 9;
const E_CODE_DRIFT: u16 = 10;

/// 码族错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperErrError {
    /// 空输入（空规格表 / 空样本集）。
    Empty,
    /// 查询的码值不在闭集内。
    UnknownCode,
    /// 同一码值被登记两次。
    Duplicate,
    /// 注册表容量已满。
    RegistryFull,
    /// 恢复策略与重试预算矛盾（AutoRetry 必须留预算，其余必须为零）。
    Policy,
    /// 事件账容量已满。
    LedgerFull,
    /// 注册表 / 事件账已封账，只读。
    Sealed,
    /// 事件本身非法（页号或纸号为 0）。
    BadEvent,
    /// 码族未凑满 15 条（缺条目视为不完整，不静默通过）。
    Incomplete,
    /// 码值落在 1..=15 之外 —— 表结构漂移。
    CodeDrift,
}

impl PaperErrError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PaperErrError::Empty => E_EMPTY,
            PaperErrError::UnknownCode => E_UNKNOWN_CODE,
            PaperErrError::Duplicate => E_DUPLICATE,
            PaperErrError::RegistryFull => E_REGISTRY_FULL,
            PaperErrError::Policy => E_POLICY,
            PaperErrError::LedgerFull => E_LEDGER_FULL,
            PaperErrError::Sealed => E_SEALED,
            PaperErrError::BadEvent => E_BAD_EVENT,
            PaperErrError::Incomplete => E_INCOMPLETE,
            PaperErrError::CodeDrift => E_CODE_DRIFT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PaperErrError::Empty => "empty paper fault table",
            PaperErrError::UnknownCode => "paper fault code outside closed set",
            PaperErrError::Duplicate => "paper fault code registered twice",
            PaperErrError::RegistryFull => "paper fault registry full",
            PaperErrError::Policy => "recovery policy contradicts retry budget",
            PaperErrError::LedgerFull => "paper fault ledger full",
            PaperErrError::Sealed => "paper fault table sealed read-only",
            PaperErrError::BadEvent => "fault event with zero page or sheet",
            PaperErrError::Incomplete => "paper fault family missing entries",
            PaperErrError::CodeDrift => "paper fault code drifted out of 1..=15",
        }
    }
}

/// 错误所属族。族的用途是**分流恢复链路**：进纸族错只关进纸辊，走纸族错
/// 必须停机开门，介质族错只需换纸，机制族错要报修。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 进纸（纸盒 / 拾纸辊）。
    Feed,
    /// 走纸（传输路径 / 定影前段）。
    Path,
    /// 尺寸（幅面 / 方向）。
    Size,
    /// 介质（类型 / 克重 / 厚度）。
    Media,
    /// 机构（分离 / 排出）。
    Mechanism,
}

impl Family {
    /// 族码 0..=4。
    pub fn code(self) -> u8 {
        match self {
            Family::Feed => 0,
            Family::Path => 1,
            Family::Size => 2,
            Family::Media => 3,
            Family::Mechanism => 4,
        }
    }

    /// 族码反解；闭集外返回 [`PaperErrError::CodeDrift`]。
    pub fn from_code(c: u8) -> Result<Family, PaperErrError> {
        match c {
            0 => Ok(Family::Feed),
            1 => Ok(Family::Path),
            2 => Ok(Family::Size),
            3 => Ok(Family::Media),
            4 => Ok(Family::Mechanism),
            _ => Err(PaperErrError::CodeDrift),
        }
    }

    /// 族名（诊断文本用）。
    pub fn label(self) -> &'static str {
        match self {
            Family::Feed => "feed",
            Family::Path => "path",
            Family::Size => "size",
            Family::Media => "media",
            Family::Mechanism => "mechanism",
        }
    }
}

/// 严重度。只增不减：一条 Fatal 事件把整份报告的上确界抬到 Fatal，
/// 后面的 Warning 无法把它压回去——上确界语义保证报告不会「被后到的轻错洗白」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 提示（不需停印）。
    Info,
    /// 警告（可能自动恢复）。
    Warning,
    /// 错误（当前作业受阻）。
    Error,
    /// 致命（必须停机开门）。
    Fatal,
}

impl Severity {
    /// 严重度码 0..=3。
    pub fn code(self) -> u8 {
        match self {
            Severity::Info => 0,
            Severity::Warning => 1,
            Severity::Error => 2,
            Severity::Fatal => 3,
        }
    }

    /// 严重度码反解；闭集外返回 [`PaperErrError::CodeDrift`]。
    pub fn from_code(c: u8) -> Result<Severity, PaperErrError> {
        match c {
            0 => Ok(Severity::Info),
            1 => Ok(Severity::Warning),
            2 => Ok(Severity::Error),
            3 => Ok(Severity::Fatal),
            _ => Err(PaperErrError::CodeDrift),
        }
    }

    /// 单调比较用的序（Info=0 … Fatal=3）。
    pub fn rank(self) -> u8 {
        self.code()
    }

    /// 严重度名。
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warn",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }
}

/// 恢复动作。错误码只描述**现象**，恢复动作才描述**处置**；两者分开
/// 是因为同一现象在不同机型上可能走不同恢复路径（面板/自动/上门）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// 自动重试（受重试预算约束）。
    AutoRetry,
    /// 面板提示操作员。
    AskOperator,
    /// 清卡纸（必须开门）。
    ClearJam,
    /// 换介质（换纸/换盒）。
    ReplaceMedia,
    /// 中止作业（不可自动恢复）。
    AbortJob,
}

impl Recovery {
    /// 恢复码 0..=4。
    pub fn code(self) -> u8 {
        match self {
            Recovery::AutoRetry => 0,
            Recovery::AskOperator => 1,
            Recovery::ClearJam => 2,
            Recovery::ReplaceMedia => 3,
            Recovery::AbortJob => 4,
        }
    }

    /// 恢复码反解；闭集外返回 [`PaperErrError::CodeDrift`]。
    pub fn from_code(c: u8) -> Result<Recovery, PaperErrError> {
        match c {
            0 => Ok(Recovery::AutoRetry),
            1 => Ok(Recovery::AskOperator),
            2 => Ok(Recovery::ClearJam),
            3 => Ok(Recovery::ReplaceMedia),
            4 => Ok(Recovery::AbortJob),
            _ => Err(PaperErrError::CodeDrift),
        }
    }

    /// 恢复动作名。
    pub fn label(self) -> &'static str {
        match self {
            Recovery::AutoRetry => "auto-retry",
            Recovery::AskOperator => "ask-operator",
            Recovery::ClearJam => "clear-jam",
            Recovery::ReplaceMedia => "replace-media",
            Recovery::AbortJob => "abort-job",
        }
    }
}

/// 纸张错误码族：15 条，码值就是枚举序号（1..=15）。
///
/// 码值一经发布不得重排——现场日志、售后脚本、面板文案都按数字索引。
/// 新增错误只能追加到 16，并且必须同步本模块、手册与判据三处。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperFault {
    /// 1 缺纸（进纸盒无介质）。
    NoPaper,
    /// 2 纸盒空（已拾到空盒信号）。
    TrayEmpty,
    /// 3 纸盒缺失（盒未装到位）。
    TrayMissing,
    /// 4 拾纸失败（有纸但未吸起）。
    TrayMisfeed,
    /// 5 进纸口卡纸。
    JamInput,
    /// 6 机内卡纸。
    JamInside,
    /// 7 出纸口卡纸。
    JamOutput,
    /// 8 尺寸不符（实际幅面与作业声明不一致）。
    SizeMismatch,
    /// 9 方向不符（纵横装反）。
    OrientationMismatch,
    /// 10 介质类型不支持。
    MediaUnsupported,
    /// 11 克重越界。
    WeightOutOfRange,
    /// 12 装纸过量（超盒容量上限）。
    TooManySheets,
    /// 13 纸张起皱（走纸后形变）。
    PaperWrinkle,
    /// 14 双张进纸（分离失败）。
    DoubleFeed,
    /// 15 排纸受阻（出口被挡）。
    EjectBlocked,
}

impl PaperFault {
    /// 全部 15 条（码序即数组序，供完整性与摘要遍历）。
    pub const ALL: [PaperFault; 15] = [
        PaperFault::NoPaper,
        PaperFault::TrayEmpty,
        PaperFault::TrayMissing,
        PaperFault::TrayMisfeed,
        PaperFault::JamInput,
        PaperFault::JamInside,
        PaperFault::JamOutput,
        PaperFault::SizeMismatch,
        PaperFault::OrientationMismatch,
        PaperFault::MediaUnsupported,
        PaperFault::WeightOutOfRange,
        PaperFault::TooManySheets,
        PaperFault::PaperWrinkle,
        PaperFault::DoubleFeed,
        PaperFault::EjectBlocked,
    ];

    /// 闭集码值 1..=15。
    pub fn code(self) -> u8 {
        match self {
            PaperFault::NoPaper => 1,
            PaperFault::TrayEmpty => 2,
            PaperFault::TrayMissing => 3,
            PaperFault::TrayMisfeed => 4,
            PaperFault::JamInput => 5,
            PaperFault::JamInside => 6,
            PaperFault::JamOutput => 7,
            PaperFault::SizeMismatch => 8,
            PaperFault::OrientationMismatch => 9,
            PaperFault::MediaUnsupported => 10,
            PaperFault::WeightOutOfRange => 11,
            PaperFault::TooManySheets => 12,
            PaperFault::PaperWrinkle => 13,
            PaperFault::DoubleFeed => 14,
            PaperFault::EjectBlocked => 15,
        }
    }

    /// 码值反解。0 与 >15 都是**表结构漂移**（不是「未知」），
    /// 区分二者能让现场一眼看出是固件版本错配还是查错了码。
    pub fn from_code(c: u8) -> Result<PaperFault, PaperErrError> {
        match c {
            1 => Ok(PaperFault::NoPaper),
            2 => Ok(PaperFault::TrayEmpty),
            3 => Ok(PaperFault::TrayMissing),
            4 => Ok(PaperFault::TrayMisfeed),
            5 => Ok(PaperFault::JamInput),
            6 => Ok(PaperFault::JamInside),
            7 => Ok(PaperFault::JamOutput),
            8 => Ok(PaperFault::SizeMismatch),
            9 => Ok(PaperFault::OrientationMismatch),
            10 => Ok(PaperFault::MediaUnsupported),
            11 => Ok(PaperFault::WeightOutOfRange),
            12 => Ok(PaperFault::TooManySheets),
            13 => Ok(PaperFault::PaperWrinkle),
            14 => Ok(PaperFault::DoubleFeed),
            15 => Ok(PaperFault::EjectBlocked),
            _ => Err(PaperErrError::CodeDrift),
        }
    }

    /// 稳定消息键（多语言文案按它索引；改动即视为文案契约变更）。
    pub fn key(self) -> &'static str {
        match self {
            PaperFault::NoPaper => "paper.no-paper",
            PaperFault::TrayEmpty => "paper.tray-empty",
            PaperFault::TrayMissing => "paper.tray-missing",
            PaperFault::TrayMisfeed => "paper.tray-misfeed",
            PaperFault::JamInput => "paper.jam-input",
            PaperFault::JamInside => "paper.jam-inside",
            PaperFault::JamOutput => "paper.jam-output",
            PaperFault::SizeMismatch => "paper.size-mismatch",
            PaperFault::OrientationMismatch => "paper.orientation-mismatch",
            PaperFault::MediaUnsupported => "paper.media-unsupported",
            PaperFault::WeightOutOfRange => "paper.weight-out-of-range",
            PaperFault::TooManySheets => "paper.too-many-sheets",
            PaperFault::PaperWrinkle => "paper.paper-wrinkle",
            PaperFault::DoubleFeed => "paper.double-feed",
            PaperFault::EjectBlocked => "paper.eject-blocked",
        }
    }

    /// 所属族。
    pub fn family(self) -> Family {
        match self {
            PaperFault::NoPaper
            | PaperFault::TrayEmpty
            | PaperFault::TrayMissing
            | PaperFault::TrayMisfeed => Family::Feed,
            PaperFault::JamInput | PaperFault::JamInside | PaperFault::JamOutput => Family::Path,
            PaperFault::SizeMismatch | PaperFault::OrientationMismatch => Family::Size,
            PaperFault::MediaUnsupported
            | PaperFault::WeightOutOfRange
            | PaperFault::TooManySheets => Family::Media,
            PaperFault::PaperWrinkle | PaperFault::DoubleFeed | PaperFault::EjectBlocked => {
                Family::Mechanism
            }
        }
    }

    /// 严重度。卡纸三类里「机内 / 出纸口」定为 Fatal：这两处用户无法
    /// 自行清理，必须停机报修；进纸口允许用户开门自清，故定 Error。
    pub fn severity(self) -> Severity {
        match self {
            PaperFault::NoPaper => Severity::Warning,
            PaperFault::TrayMisfeed => Severity::Error,
            PaperFault::OrientationMismatch => Severity::Warning,
            PaperFault::WeightOutOfRange => Severity::Warning,
            PaperFault::PaperWrinkle => Severity::Warning,
            PaperFault::JamInside | PaperFault::JamOutput | PaperFault::EjectBlocked => {
                Severity::Fatal
            }
            _ => Severity::Error,
        }
    }

    /// 恢复动作。
    pub fn recovery(self) -> Recovery {
        match self {
            PaperFault::NoPaper => Recovery::AutoRetry,
            PaperFault::TrayEmpty
            | PaperFault::TrayMissing
            | PaperFault::MediaUnsupported
            | PaperFault::WeightOutOfRange
            | PaperFault::TooManySheets => Recovery::AskOperator,
            PaperFault::TrayMisfeed | PaperFault::OrientationMismatch => Recovery::AutoRetry,
            PaperFault::SizeMismatch => Recovery::ReplaceMedia,
            PaperFault::JamInput
            | PaperFault::JamInside
            | PaperFault::JamOutput
            | PaperFault::PaperWrinkle
            | PaperFault::DoubleFeed
            | PaperFault::EjectBlocked => Recovery::ClearJam,
        }
    }

    /// 自动重试预算。只有 AutoRetry 允许非零：重试一条注定失败的路径
    /// 只会把卡纸从「一张」变成「一叠」，所以预算必须显式且在策略上自洽。
    pub fn retry_budget(self) -> u8 {
        match self {
            PaperFault::NoPaper => 2,
            PaperFault::TrayMisfeed => 1,
            PaperFault::OrientationMismatch => 1,
            _ => 0,
        }
    }

    /// 该错误码的完整规格。
    pub fn spec(self) -> FaultSpec {
        FaultSpec {
            code: self.code(),
            key: self.key(),
            family: self.family(),
            severity: self.severity(),
            recovery: self.recovery(),
            retry_budget: self.retry_budget(),
        }
    }
}

/// 单条错误码规格（四个事实字段的载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultSpec {
    pub code: u8,
    pub key: &'static str,
    pub family: Family,
    pub severity: Severity,
    pub recovery: Recovery,
    pub retry_budget: u8,
}

impl FaultSpec {
    /// 规格自检：码域 + 策略 + 键前缀。
    pub fn validate(&self) -> Result<(), PaperErrError> {
        if self.code == 0 || self.code > 15 {
            return Err(PaperErrError::CodeDrift);
        }
        if self.key.is_empty() || !self.key.starts_with("paper.") {
            return Err(PaperErrError::Policy);
        }
        let auto = self.recovery == Recovery::AutoRetry;
        if auto != (self.retry_budget >= 1) {
            return Err(PaperErrError::Policy);
        }
        Ok(())
    }

    /// 一行可读规格（日志用）。
    pub fn line(&self) -> String {
        format!(
            "#{} {} {}/{}/{} retry={}",
            self.code,
            self.key,
            self.family.label(),
            self.severity.label(),
            self.recovery.label(),
            self.retry_budget
        )
    }
}

// ---------------------------------------------------------------------------
// 摘要（零漂移的唯一权威）
// ---------------------------------------------------------------------------

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_str(h: u32, s: &str) -> u32 {
    let bs = s.as_bytes();
    let mut h = h;
    let mut i = 0usize;
    while i < bs.len() {
        if let Some(&b) = bs.get(i) {
            h = fnv_byte(h, b);
        }
        i += 1;
    }
    h
}

fn fold_row(h: u32, code: u8, key: &str, fam: u8, sev: u8, rec: u8, budget: u8) -> u32 {
    let h = fnv_byte(h, code);
    let h = fnv_str(h, key);
    let h = fnv_byte(h, fam);
    let h = fnv_byte(h, sev);
    let h = fnv_byte(h, rec);
    fnv_byte(h, budget)
}

/// 15 条码表的冻结摘要。任何字段改动都会让 J1 当红——这是「零漂移」的实现。
pub fn canonical_digest() -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i) {
            let s = f.spec();
            h = fold_row(
                h,
                s.code,
                s.key,
                s.family.code(),
                s.severity.code(),
                s.recovery.code(),
                s.retry_budget,
            );
        }
        i += 1;
    }
    h
}

/// 独立参考摘要：用字面量表（不经枚举访问器）重算同一张表。
///
/// 与 [`canonical_digest`] 是两条独立推导——前者读枚举，后者读数据——
/// 两者必须相等，否则说明枚举的某个访问器写错了分支。
pub fn reference_digest() -> u32 {
    // (code, key, family, severity, recovery, budget)
    const ROWS: [(u8, &str, u8, u8, u8, u8); 15] = [
        (1, "paper.no-paper", 0, 1, 0, 2),
        (2, "paper.tray-empty", 0, 2, 1, 0),
        (3, "paper.tray-missing", 0, 2, 1, 0),
        (4, "paper.tray-misfeed", 0, 2, 0, 1),
        (5, "paper.jam-input", 1, 2, 2, 0),
        (6, "paper.jam-inside", 1, 3, 2, 0),
        (7, "paper.jam-output", 1, 3, 2, 0),
        (8, "paper.size-mismatch", 2, 2, 3, 0),
        (9, "paper.orientation-mismatch", 2, 1, 0, 1),
        (10, "paper.media-unsupported", 3, 2, 1, 0),
        (11, "paper.weight-out-of-range", 3, 1, 1, 0),
        (12, "paper.too-many-sheets", 3, 2, 1, 0),
        (13, "paper.paper-wrinkle", 4, 1, 2, 0),
        (14, "paper.double-feed", 4, 2, 2, 0),
        (15, "paper.eject-blocked", 4, 3, 2, 0),
    ];
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < ROWS.len() {
        if let Some(r) = ROWS.get(i) {
            h = fold_row(h, r.0, r.1, r.2, r.3, r.4, r.5);
        }
        i += 1;
    }
    h
}

/// 冻结摘要常量（由 [`reference_digest`] 离线算出后钉死）。
pub const FROZEN_DIGEST: u32 = 0xd2cf_5f11;

// ---------------------------------------------------------------------------
// 规格注册表
// ---------------------------------------------------------------------------

/// 错误码注册表。定容 + 去重 + 封账三步是**同一件事**的三个面：
/// 现场只允许「按固件里那张表」注册，多一条少一条都属于固件错配。
#[derive(Clone, Debug)]
pub struct FaultRegistry {
    entries: Vec<FaultSpec>,
    cap: usize,
    sealed: bool,
}

impl FaultRegistry {
    /// 建空表；容量 0 视为无意义入参（[`PaperErrError::RegistryFull`] 在首次登记时给出）。
    pub fn new(cap: usize) -> FaultRegistry {
        FaultRegistry {
            entries: Vec::with_capacity(cap),
            cap,
            sealed: false,
        }
    }

    /// 由一批规格建表；空批次返回 [`PaperErrError::Empty`]。
    pub fn from_specs(specs: &[FaultSpec]) -> Result<FaultRegistry, PaperErrError> {
        if specs.is_empty() {
            return Err(PaperErrError::Empty);
        }
        let mut r = FaultRegistry::new(specs.len());
        let mut i = 0usize;
        while i < specs.len() {
            if let Some(s) = specs.get(i) {
                r.register(*s)?;
            }
            i += 1;
        }
        Ok(r)
    }

    /// 登记一条规格：封账 / 定容 / 去重 / 自检，四道闸按代价从小到大排。
    pub fn register(&mut self, spec: FaultSpec) -> Result<(), PaperErrError> {
        if self.sealed {
            return Err(PaperErrError::Sealed);
        }
        if self.entries.len() >= self.cap {
            return Err(PaperErrError::RegistryFull);
        }
        spec.validate()?;
        if self.lookup(spec.code).is_ok() {
            return Err(PaperErrError::Duplicate);
        }
        self.entries.push(spec);
        Ok(())
    }

    /// 按码值查规格。
    pub fn lookup(&self, code: u8) -> Result<&FaultSpec, PaperErrError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(s) = self.entries.get(i) {
                if s.code == code {
                    return Ok(s);
                }
            }
            i += 1;
        }
        Err(PaperErrError::UnknownCode)
    }

    /// 把 15 条标准规格一次性登记（返回条数）。
    pub fn register_all(&mut self) -> Result<usize, PaperErrError> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < PaperFault::ALL.len() {
            if let Some(f) = PaperFault::ALL.get(i) {
                self.register(f.spec())?;
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 完整性：正好 15 条且码 1..=15 各一条。
    pub fn complete(&self) -> bool {
        if self.entries.len() != 15 {
            return false;
        }
        let mut c: u8 = 1;
        while c <= 15 {
            if self.lookup(c).is_err() {
                return false;
            }
            c += 1;
        }
        true
    }

    /// 要求完整；缺条目返回 [`PaperErrError::Incomplete`]。
    pub fn require_complete(&self) -> Result<usize, PaperErrError> {
        if self.complete() {
            Ok(self.entries.len())
        } else {
            Err(PaperErrError::Incomplete)
        }
    }

    /// 封账（此后只读）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }
}

// ---------------------------------------------------------------------------
// 故障事件账
// ---------------------------------------------------------------------------

/// 一条故障事件。sheet=0 表示「发生在装纸阶段，尚未形成纸张实体」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultEvent {
    pub code: u8,
    pub page: u16,
    pub sheet: u16,
}

/// 故障事件账：定容 + 逐码计数 + 封账。
///
/// 计数用定长数组而不是 map：码域是 1..=15 的闭集，定长数组天然把
/// 「未知码」挡在数组外（下标越界前就被 `from_code` 拒了），且零分配。
#[derive(Clone, Debug)]
pub struct FaultLedger {
    events: Vec<FaultEvent>,
    counters: [u32; 16],
    cap: usize,
    sealed: bool,
    rejected: u32,
}

impl FaultLedger {
    /// 建空账本。
    pub fn new(cap: usize) -> FaultLedger {
        FaultLedger {
            events: Vec::with_capacity(cap),
            counters: [0u32; 16],
            cap,
            sealed: false,
            rejected: 0,
        }
    }

    /// 记一条事件：封账 → 码域 → 事件合法性 → 容量。
    pub fn record(&mut self, code: u8, page: u16, sheet: u16) -> Result<(), PaperErrError> {
        if self.sealed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PaperErrError::Sealed);
        }
        if PaperFault::from_code(code).is_err() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PaperErrError::UnknownCode);
        }
        if page == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PaperErrError::BadEvent);
        }
        if self.events.len() >= self.cap {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PaperErrError::LedgerFull);
        }
        let idx = code as usize;
        if let Some(c) = self.counters.get_mut(idx) {
            *c = c.saturating_add(1);
            self.events.push(FaultEvent { code, page, sheet });
            Ok(())
        } else {
            Err(PaperErrError::CodeDrift)
        }
    }

    /// 单码计数。
    pub fn count(&self, code: u8) -> u32 {
        match self.counters.get(code as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 已记录事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 被拒次数（显性记账，不静默）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 全账计数之和（守恒判据的右侧）。
    pub fn total_counted(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.counters.len() {
            if let Some(v) = self.counters.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }

    /// 严重度上确界：扫事件取最大 rank，空账是 Info。
    pub fn max_severity(&self) -> Severity {
        let mut best = Severity::Info;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                if let Ok(f) = PaperFault::from_code(e.code) {
                    if f.severity().rank() > best.rank() {
                        best = f.severity();
                    }
                }
            }
            i += 1;
        }
        best
    }

    /// 顺序摘要：事件序列（码/页/纸）逐字节折叠，用于复跑一致性。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = fnv_byte(h, e.code);
                h = fnv_byte(h, (e.page >> 8) as u8);
                h = fnv_byte(h, e.page as u8);
                h = fnv_byte(h, (e.sheet >> 8) as u8);
                h = fnv_byte(h, e.sheet as u8);
            }
            i += 1;
        }
        h
    }

    /// 出报告（不可失败：账本本身就是事实）。
    pub fn report(&self) -> FaultReport {
        let mut fatal = 0u32;
        let mut errors = 0u32;
        let mut first = 0u8;
        let mut last = 0u8;
        if let Some(e) = self.events.first() {
            first = e.code;
        }
        if let Some(e) = self.events.last() {
            last = e.code;
        }
        let mut c: u8 = 1;
        while c <= 15 {
            if let Ok(f) = PaperFault::from_code(c) {
                match f.severity() {
                    Severity::Fatal => fatal = fatal.saturating_add(self.count(c)),
                    Severity::Error => errors = errors.saturating_add(self.count(c)),
                    _ => {}
                }
            }
            c += 1;
        }
        FaultReport {
            total: self.events.len() as u32,
            fatal,
            errors,
            max_severity: self.max_severity(),
            first,
            last,
            digest: self.digest(),
        }
    }
}

/// 故障汇总（面板/日志共用的只读快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultReport {
    pub total: u32,
    pub fatal: u32,
    pub errors: u32,
    pub max_severity: Severity,
    pub first: u8,
    pub last: u8,
    pub digest: u32,
}

impl FaultReport {
    /// 是否必须停机（只有 Fatal 才拦作业）。
    pub fn must_stop(self) -> bool {
        self.max_severity == Severity::Fatal
    }

    /// 一行可读摘要。
    pub fn line(self) -> String {
        format!(
            "total={} fatal={} err={} max={} {}..{}",
            self.total,
            self.fatal,
            self.errors,
            self.max_severity.label(),
            self.first,
            self.last
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核：6 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 6 类坏样本注入核，返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1) 空规格表。
    if FaultRegistry::from_specs(&[]).err() == Some(PaperErrError::Empty) {
        caught += 1;
    }

    // 2) 同一码登记两次。
    let mut dup = FaultRegistry::new(4);
    let a = PaperFault::NoPaper.spec();
    if dup.register(a).is_ok() && dup.register(a).err() == Some(PaperErrError::Duplicate) {
        caught += 1;
    }

    // 3) 容量不足（cap=2 收 3 条）。
    let mut tiny = FaultRegistry::new(2);
    let ok2 = tiny.register(PaperFault::NoPaper.spec()).is_ok()
        && tiny.register(PaperFault::JamInput.spec()).is_ok();
    if ok2 && tiny.register(PaperFault::JamInside.spec()).err() == Some(PaperErrError::RegistryFull)
    {
        caught += 1;
    }

    // 4) 策略矛盾：AutoRetry 但预算为 0。
    let bad_policy = FaultSpec {
        code: 4,
        key: "paper.tray-misfeed",
        family: Family::Feed,
        severity: Severity::Error,
        recovery: Recovery::AutoRetry,
        retry_budget: 0,
    };
    if bad_policy.validate().err() == Some(PaperErrError::Policy) {
        caught += 1;
    }

    // 5) 账本记录未登记码（16 不在闭集）。
    let mut led = FaultLedger::new(8);
    if led.record(16, 1, 1).err() == Some(PaperErrError::UnknownCode) {
        caught += 1;
    }

    // 6) 封账后再写。
    let mut sealed = FaultLedger::new(8);
    sealed.seal();
    if sealed.record(1, 1, 1).err() == Some(PaperErrError::Sealed) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50072 自检：17 条判据。
pub fn run_unx50072_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50072");

    // J1-1 码域 ×15：条数 15、码值互异且恰好覆盖 1..=15。
    let mut codes_ok = PaperFault::ALL.len() == 15;
    let mut seen = [false; 16];
    let mut i = 0usize;
    while i < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i) {
            let c = f.code();
            if c == 0 || c > 15 {
                codes_ok = false;
            } else if let Some(slot) = seen.get_mut(c as usize) {
                if *slot {
                    codes_ok = false;
                } else {
                    *slot = true;
                }
            } else {
                codes_ok = false;
            }
        }
        i += 1;
    }
    s.add("50072-J1-码域15闭集", codes_ok, if codes_ok { "" } else { "码值重复或越界" });

    // J1-2 码 ↔ 枚举反解往返 ×15（零漂移的第一半）。
    let mut rt_ok = true;
    let mut i2 = 0usize;
    while i2 < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i2) {
            if PaperFault::from_code(f.code()).ok() != Some(*f) {
                rt_ok = false;
            }
        }
        i2 += 1;
    }
    s.add("50072-J1-码反解往返", rt_ok, if rt_ok { "" } else { "反解与枚举不一致" });

    // J1-3 规格四元组 ×15：族/严重度/恢复/预算逐项与注册表一致。
    let mut reg = FaultRegistry::new(16);
    let registered = match reg.register_all() {
        Ok(n) => n,
        Err(_) => 0,
    };
    let mut spec_ok = registered == 15;
    let mut i3 = 0usize;
    while i3 < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i3) {
            match reg.lookup(f.code()) {
                Ok(sp) => {
                    if sp.family != f.family()
                        || sp.severity != f.severity()
                        || sp.recovery != f.recovery()
                        || sp.retry_budget != f.retry_budget()
                        || sp.key != f.key()
                    {
                        spec_ok = false;
                    }
                }
                Err(_) => spec_ok = false,
            }
        }
        i3 += 1;
    }
    s.add("50072-J1-规格15一致", spec_ok, if spec_ok { "" } else { "注册表与枚举不符" });

    // J1-4 冻结摘要：枚举路径 == 字面量路径 == 钉死常量（零漂移的第二半）。
    let canon = canonical_digest();
    let refr = reference_digest();
    let ok = canon == refr && canon == FROZEN_DIGEST;
    let det = format!("{:#010x}/{:#010x}", canon, refr);
    s.add_dyn("50072-J1-冻结摘要", ok, &det);

    // J1-5 摘要敏感性：改一个预算必须改摘要（防「常量 stub」）。
    let mut h = FNV_OFFSET;
    let mut i5 = 0usize;
    while i5 < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i5) {
            let sp = f.spec();
            let budget = if sp.code == 15 { 1u8 } else { sp.retry_budget };
            h = fold_row(
                h,
                sp.code,
                sp.key,
                sp.family.code(),
                sp.severity.code(),
                sp.recovery.code(),
                budget,
            );
        }
        i5 += 1;
    }
    let ok = h != canon;
    s.add("50072-J1-摘要敏感", ok, if ok { "" } else { "改字段摘要未变" });

    // J1-6 策略自洽 ×15：AutoRetry ⟺ 预算≥1，其余预算为 0。
    let mut policy_ok = true;
    let mut i6 = 0usize;
    while i6 < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i6) {
            let sp = f.spec();
            if sp.validate().is_err() {
                policy_ok = false;
            }
            let auto = sp.recovery == Recovery::AutoRetry;
            if auto != (sp.retry_budget >= 1) {
                policy_ok = false;
            }
        }
        i6 += 1;
    }
    s.add("50072-J1-策略自洽", policy_ok, if policy_ok { "" } else { "重试预算与恢复矛盾" });

    // J1-7 严重度下界：所有码 ≥ Warning；走纸族（卡纸）≥ Error；机内/出纸/排出 = Fatal。
    let mut sev_ok = true;
    let mut i7 = 0usize;
    while i7 < PaperFault::ALL.len() {
        if let Some(f) = PaperFault::ALL.get(i7) {
            if f.severity().rank() < Severity::Warning.rank() {
                sev_ok = false;
            }
            if f.family() == Family::Path && f.severity().rank() < Severity::Error.rank() {
                sev_ok = false;
            }
        }
        i7 += 1;
    }
    if PaperFault::JamInside.severity() != Severity::Fatal
        || PaperFault::JamOutput.severity() != Severity::Fatal
        || PaperFault::EjectBlocked.severity() != Severity::Fatal
    {
        sev_ok = false;
    }
    s.add("50072-J1-严重度下界", sev_ok, if sev_ok { "" } else { "严重度低于族下界" });

    // J1-8 重复登记必拒且表不变。
    let mut dup = FaultRegistry::new(8);
    let a = PaperFault::NoPaper.spec();
    let ok = dup.register(a).is_ok()
        && dup.register(a).err() == Some(PaperErrError::Duplicate)
        && dup.len() == 1;
    s.add("50072-J1-重码必拒", ok, if ok { "" } else { "同码进了表" });

    // J1-9 定容必拒：cap=2 第三条 RegistryFull，条数停在 2。
    let mut tiny = FaultRegistry::new(2);
    let ok = tiny.register(PaperFault::NoPaper.spec()).is_ok()
        && tiny.register(PaperFault::JamInput.spec()).is_ok()
        && tiny.register(PaperFault::JamInside.spec()).err() == Some(PaperErrError::RegistryFull)
        && tiny.len() == tiny.capacity();
    s.add("50072-J1-定容必拒", ok, if ok { "" } else { "超容被接受" });

    // J1-10 漂移与未知必须区分：from_code(0)/from_code(16) = CodeDrift，lookup(99) = UnknownCode。
    let drift = FaultSpec {
        code: 16,
        key: "paper.bogus",
        family: Family::Feed,
        severity: Severity::Error,
        recovery: Recovery::AskOperator,
        retry_budget: 0,
    };
    let ok = PaperFault::from_code(0).err() == Some(PaperErrError::CodeDrift)
        && PaperFault::from_code(16).err() == Some(PaperErrError::CodeDrift)
        && drift.validate().err() == Some(PaperErrError::CodeDrift)
        && reg.lookup(99).err() == Some(PaperErrError::UnknownCode)
        && Family::from_code(5).err() == Some(PaperErrError::CodeDrift);
    s.add("50072-J1-漂移区别", ok, if ok { "" } else { "漂移与未知混淆" });

    // J1-11 不完整必报：14 条表 require_complete = Incomplete。
    let mut partial = FaultRegistry::new(16);
    let mut k = 0usize;
    while k < 14 {
        if let Some(f) = PaperFault::ALL.get(k) {
            let _ = partial.register(f.spec());
        }
        k += 1;
    }
    let ok = !partial.complete()
        && partial.require_complete().err() == Some(PaperErrError::Incomplete)
        && reg.require_complete() == Ok(15);
    s.add("50072-J1-完整性", ok, if ok { "" } else { "缺条目未报" });

    // J1-12 计数守恒：逐码计数之和 == 事件数 == 记录成功次数，且逐码可手算。
    let mut led = FaultLedger::new(64);
    let mut written = 0usize;
    let plan = [
        (1u8, 1u16),
        (6, 4),
        (6, 5),
        (13, 7),
        (6, 9),
        (1, 11),
        (15, 12),
    ];
    let mut j = 0usize;
    while j < plan.len() {
        if let Some((c, pg)) = plan.get(j) {
            if led.record(*c, *pg, (*pg).saturating_add(1)).is_ok() {
                written += 1;
            }
        }
        j += 1;
    }
    let ok = led.len() == written
        && led.total_counted() == written as u32
        && led.count(6) == 3
        && led.count(1) == 2
        && led.count(13) == 1
        && led.count(2) == 0;
    let det = format!("n={} sum={}", led.len(), led.total_counted());
    s.add_dyn("50072-J1-计数守恒", ok, &det);

    // J1-13 非法事件必拒：未登记码 / 页号 0 / 封账后写，且账不变。
    let mut rej = FaultLedger::new(8);
    let ok = rej.record(99, 1, 1).err() == Some(PaperErrError::UnknownCode)
        && rej.record(1, 0, 1).err() == Some(PaperErrError::BadEvent)
        && rej.len() == 0
        && rej.rejected() == 2;
    rej.seal();
    let ok2 = ok
        && rej.record(1, 1, 1).err() == Some(PaperErrError::Sealed)
        && rej.len() == 0
        && rej.is_sealed();
    s.add("50072-J1-非法事件", ok2, if ok2 { "" } else { "坏事件进账" });

    // J1-14 账本定容：cap=2 第三条 LedgerFull，计数不虚增。
    let mut full = FaultLedger::new(2);
    let ok = full.record(1, 1, 1).is_ok()
        && full.record(2, 2, 2).is_ok()
        && full.record(3, 3, 3).err() == Some(PaperErrError::LedgerFull)
        && full.len() == 2
        && full.count(3) == 0
        && full.total_counted() == 2;
    s.add("50072-J1-账本定容", ok, if ok { "" } else { "超容计数被篡改" });

    // J1-15 报告上确界：空账 Info；含机内卡纸后 Fatal；fatal 计数按 Fatal 码累加。
    let empty = FaultLedger::new(4).report();
    let rep = led.report();
    let ok = empty.max_severity == Severity::Info
        && !empty.must_stop()
        && rep.max_severity == Severity::Fatal
        && rep.must_stop()
        && rep.fatal == 4
        && rep.first == 1
        && rep.last == 15;
    let det = format!("fatal={} first={} last={}", rep.fatal, rep.first, rep.last);
    s.add_dyn("50072-J1-报告上确界", ok, &det);

    // J1-16 摘要确定 + 序列敏感：同序两账同摘要；换序必不同。
    let mut a1 = FaultLedger::new(8);
    let mut a2 = FaultLedger::new(8);
    let mut b1 = FaultLedger::new(8);
    let _ = a1.record(1, 1, 1);
    let _ = a1.record(6, 2, 2);
    let _ = a2.record(1, 1, 1);
    let _ = a2.record(6, 2, 2);
    let _ = b1.record(6, 2, 2);
    let _ = b1.record(1, 1, 1);
    let ok = a1.digest() == a2.digest() && a1.digest() != b1.digest();
    s.add("50072-J1-摘要敏感序", ok, if ok { "" } else { "摘要忽略顺序" });

    // J1-17 六类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50072-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50072_checks_green() {
        let set = run_unx50072_checks();
        assert!(set.all_passed(), "UNX-F50072 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50072_fault_table_is_fifteen_and_frozen() {
        assert_eq!(PaperFault::ALL.len(), 15);
        assert_eq!(canonical_digest(), reference_digest());
        assert_eq!(canonical_digest(), FROZEN_DIGEST);
        let mut c: u8 = 1;
        while c <= 15 {
            assert_eq!(PaperFault::from_code(c).map(|f| f.code()).ok(), Some(c));
            c += 1;
        }
    }

    #[test]
    fn unx50072_ledger_conserves() {
        let mut led = FaultLedger::new(16);
        let mut n = 0u32;
        for pg in 1..=8u16 {
            if led.record(6, pg, pg).is_ok() {
                n += 1;
            }
        }
        assert_eq!(led.len() as u32, n);
        assert_eq!(led.total_counted(), n);
        assert_eq!(led.count(6), n);
        assert_eq!(led.report().max_severity, Severity::Fatal);
    }

    #[test]
    fn unx50072_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
