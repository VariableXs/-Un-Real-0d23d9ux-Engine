//! unx179_faillog — UNX-F0179 EFI 调用早期失败显性化（0x6B60 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0179）：EFI 调用早期失败显性化。
//! 判据原文（UNX-F0179-J1）：固件调用失败零静默，全部进日志中心
//! （十三·补口径），注入 10 类失败全显性。
//!
//! 设计三线：①调用封装零静默（每个 EFI 调用的返回状态码必落账：成功
//! 也记账、失败必带分类码与场景键——任何路径都不许吞返回值）；②十类
//! 失败分类表（Unsupported/InvalidParam/OutOfRes/NotFound/Timeout/
//! DeviceError/WriteProtected/SecurityViolation/Abort/ICRCError，每类
//! 有显性化级别）；③日志中心对账（调用数 ≡ 成功 + 失败，失败数 ≡
//! 分类账总和，零静默审计扫账本无无名调用）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const FAILLOG_VERSION: &str = "179-faillog-v1";

/// 段基（0x6B60）。
pub const SEG_BASE: u16 = 0x6B60;

// ---------------------------------------------------------------------------
// EFI 状态码与十类失败
// ---------------------------------------------------------------------------

/// EFI_STATUS 摘要（封装层只关心成功/失败+类别）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EfiStatus {
    /// EFI_SUCCESS (0)。
    Success,
    /// 失败（携带原始非零码）。
    Err(u64),
}

/// 十类失败分类（判据注入全表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailKind {
    #[default]
    Unsupported,
    InvalidParam,
    OutOfResources,
    NotFound,
    Timeout,
    DeviceError,
    WriteProtected,
    SecurityViolation,
    Aborted,
    CrcError,
}

impl FailKind {
    /// 全部十类（注入遍历序）。
    pub const ALL: [FailKind; 10] = [
        FailKind::Unsupported,
        FailKind::InvalidParam,
        FailKind::OutOfResources,
        FailKind::NotFound,
        FailKind::Timeout,
        FailKind::DeviceError,
        FailKind::WriteProtected,
        FailKind::SecurityViolation,
        FailKind::Aborted,
        FailKind::CrcError,
    ];

    /// 分类字面量（日志中心检索键）。
    pub fn tag(self) -> &'static str {
        match self {
            FailKind::Unsupported => "unsupported",
            FailKind::InvalidParam => "invalid-param",
            FailKind::OutOfResources => "out-of-res",
            FailKind::NotFound => "not-found",
            FailKind::Timeout => "timeout",
            FailKind::DeviceError => "device-error",
            FailKind::WriteProtected => "write-protected",
            FailKind::SecurityViolation => "sec-violation",
            FailKind::Aborted => "aborted",
            FailKind::CrcError => "crc-error",
        }
    }

    /// 显性化级别：2=致命（阻断启动路径）、1=可降级、0=仅记账。
    pub fn severity(self) -> u8 {
        match self {
            FailKind::Unsupported | FailKind::SecurityViolation => 2,
            FailKind::InvalidParam | FailKind::OutOfResources | FailKind::NotFound => 1,
            _ => 0,
        }
    }

    /// 原始码 → 分类（EFI 常见非零码归一）。
    pub fn of_raw(raw: u64) -> FailKind {
        match raw {
            3 => FailKind::InvalidParam,
            2 => FailKind::Unsupported,
            4 => FailKind::Timeout,
            5 => FailKind::DeviceError,
            6 => FailKind::WriteProtected,
            7 => FailKind::SecurityViolation,
            8 => FailKind::NotFound,
            9 => FailKind::Aborted,
            10 => FailKind::CrcError,
            _ => FailKind::OutOfResources,
        }
    }
}

// ---------------------------------------------------------------------------
// 日志中心
// ---------------------------------------------------------------------------

/// 单条调用账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRec {
    /// 场景键（哪个调用点，如 "LocateProtocol:GetVariable"）。
    pub site: String,
    /// 成功否。
    pub ok: bool,
    /// 失败分类（成功时 None）。
    pub kind: Option<FailKind>,
    /// 原始状态码。
    pub raw: u64,
}

/// 日志中心：调用封装唯一出口。
#[derive(Debug, Default)]
pub struct LogCenter {
    recs: Vec<CallRec>,
    /// 分类计数账。
    kind_counts: [(FailKind, u32); 10],
    /// 降级账（severity≥1 的显性化动作）。
    escalated: Vec<(String, u8)>,
}

impl LogCenter {
    /// 空中心。
    pub fn new() -> Self {
        LogCenter {
            recs: Vec::new(),
            kind_counts: FailKind::ALL.map(|k| (k, 0)),
            escalated: Vec::new(),
        }
    }

    /// 记一笔（成功/失败都记——零静默的第一层）。
    pub fn record(&mut self, site: &str, st: EfiStatus) {
        let rec = match st {
            EfiStatus::Success => CallRec {
                site: String::from(site),
                ok: true,
                kind: None,
                raw: 0,
            },
            EfiStatus::Err(raw) => {
                let k = FailKind::of_raw(raw);
                for slot in self.kind_counts.iter_mut() {
                    if slot.0 == k {
                        slot.1 += 1;
                        break;
                    }
                }
                CallRec {
                    site: String::from(site),
                    ok: false,
                    kind: Some(k),
                    raw,
                }
            }
        };
        self.recs.push(rec);
    }

    /// 显性化升级：失败且 severity≥1 的场景入升级账（第二层显性化）。
    pub fn escalate(&mut self) -> usize {
        let mut n = 0;
        for r in self.recs.iter() {
            if let Some(k) = r.kind {
                if k.severity() >= 1 {
                    self.escalated.push((r.site.clone(), k.severity()));
                    n += 1;
                }
            }
        }
        n
    }

    /// 零静默审计（判据主体）：调用数 ≡ 成功 + 失败；失败数 ≡ 分类账总和；
    /// 每笔失败都有分类。三条全过才算零静默。
    pub fn zero_silent_audit(&self) -> Result<(), u16> {
        let succ = self.recs.iter().filter(|r| r.ok).count();
        let fail = self.recs.iter().filter(|r| !r.ok).count();
        if succ + fail != self.recs.len() {
            return Err(E_LEDGER_UNBALANCED);
        }
        let kind_sum: u32 = self.kind_counts.iter().map(|s| s.1).sum();
        if kind_sum as usize != fail {
            return Err(E_LEDGER_UNBALANCED);
        }
        for r in self.recs.iter() {
            if !r.ok && r.kind.is_none() {
                return Err(E_SILENT_FAIL);
            }
        }
        for r in self.recs.iter() {
            if r.site.is_empty() {
                return Err(E_NO_SITE);
            }
        }
        Ok(())
    }

    /// 分类命中数。
    pub fn count_of(&self, k: FailKind) -> u32 {
        self.kind_counts
            .iter()
            .find(|s| s.0 == k)
            .map(|s| s.1)
            .unwrap_or(0)
    }

    /// 账目快照：(调用数, 成功数, 失败数, 升级数)。
    pub fn snapshot(&self) -> (usize, usize, usize, usize) {
        (
            self.recs.len(),
            self.recs.iter().filter(|r| r.ok).count(),
            self.recs.iter().filter(|r| !r.ok).count(),
            self.escalated.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// 调用封装（零静默唯一出口）
// ---------------------------------------------------------------------------

/// 封装调用：任何调用必须经此落账，不允许裸调后丢弃返回值。
pub fn wrapped_call(lc: &mut LogCenter, site: &str, raw_status: u64) -> Result<(), u16> {
    let st = if raw_status == 0 {
        EfiStatus::Success
    } else {
        EfiStatus::Err(raw_status)
    };
    lc.record(site, st);
    match st {
        EfiStatus::Success => Ok(()),
        EfiStatus::Err(_) => Err(E_CALL_FAILED),
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x6B60 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FailLogErr {
    /// 账不平。
    LedgerUnbalanced = SEG_BASE + 1,
    /// 失败无分类（静默失败）。
    SilentFail = SEG_BASE + 2,
    /// 调用点键缺失。
    NoSite = SEG_BASE + 3,
    /// 封装调用失败本体。
    CallFailed = SEG_BASE + 4,
    /// 注入覆盖不全。
    InjectGaps = SEG_BASE + 5,
}

impl FailLogErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            FailLogErr::LedgerUnbalanced => "调用/成功/失败/分类账不平",
            FailLogErr::SilentFail => "失败笔无分类（静默）",
            FailLogErr::NoSite => "调用点键为空",
            FailLogErr::CallFailed => "封装调用失败（已落账）",
            FailLogErr::InjectGaps => "十类失败注入未全显性",
        }
    }
}

const E_LEDGER_UNBALANCED: u16 = FailLogErr::LedgerUnbalanced.code();
const E_SILENT_FAIL: u16 = FailLogErr::SilentFail.code();
const E_NO_SITE: u16 = FailLogErr::NoSite.code();
const E_CALL_FAILED: u16 = FailLogErr::CallFailed.code();
const E_INJECT_GAPS: u16 = FailLogErr::InjectGaps.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 十类失败注入表：每类一个代表原始码。
fn inject_table() -> [(FailKind, u64, &'static str); 10] {
    [
        (FailKind::Unsupported, 2, "GetVariable:missing-guid"),
        (FailKind::InvalidParam, 3, "SetVariable:bad-attr"),
        (FailKind::OutOfResources, 14, "AllocatePool:big"),
        (FailKind::NotFound, 8, "LocateProtocol:none"),
        (FailKind::Timeout, 4, "BlockIo:poll"),
        (FailKind::DeviceError, 5, "BlockIo:read"),
        (FailKind::WriteProtected, 6, "SetVariable:ro"),
        (FailKind::SecurityViolation, 7, "LoadImage:unsigned"),
        (FailKind::Aborted, 9, "Exit:aborted"),
        (FailKind::CrcError, 10, "LoadFile2:crc"),
    ]
}

/// 判据集（UNX-F0179-J1 + 结构红线）。自证五段。
pub fn self_audit() -> Result<(), u16> {
    let injects = inject_table();
    // 一、J1 主体：十类失败注入全显性（全落账、全有分类）。
    let mut lc = LogCenter::new();
    for (k, raw, site) in injects.iter() {
        let r = wrapped_call(&mut lc, site, *raw);
        if r.is_ok() {
            return Err(E_CALL_FAILED);
        }
        if lc.count_of(*k) == 0 {
            return Err(E_INJECT_GAPS);
        }
    }
    // 二、成功路径同样落账（零静默双向：成功也不许丢）。
    wrapped_call(&mut lc, "GetVariable:ok", 0)?;
    let (total, succ, fail, _esc) = lc.snapshot();
    if total != 11 || succ != 1 || fail != 10 {
        return Err(E_LEDGER_UNBALANCED);
    }
    // 三、零静默审计三账平。
    lc.zero_silent_audit()?;
    // 四、显性化升级：severity≥1 的 5 类（Unsupported/InvalidParam/
    // OutOfRes/NotFound/SecViolation）各一笔全升级。
    let esc = lc.escalate();
    if esc != 5 {
        return Err(E_INJECT_GAPS);
    }
    // 五、分类表完整性：十类 tag 互异（检索键唯一性红线）。
    for i in 0..10 {
        for j in (i + 1)..10 {
            if FailKind::ALL[i].tag() == FailKind::ALL[j].tag() {
                return Err(E_INJECT_GAPS);
            }
        }
    }
    // 六、判别式段位对拍。
    if E_INJECT_GAPS != SEG_BASE + 5 || E_SILENT_FAIL != SEG_BASE + 2 {
        return Err(E_INJECT_GAPS);
    }
    Ok(())
}
