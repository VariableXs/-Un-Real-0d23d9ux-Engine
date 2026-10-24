//! unx180_ktestb09 — UNX-F0180 ktest 引导面 B09 批断言集（0x6B70 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0180）：ktest 引导面 B09 批断言集。
//! 判据原文（UNX-F0180-J1）：本批判据聚合入 ktest，一次命令全绿。
//!
//! 设计三线：①断言注册表（B09 批——UNX-F0166~F0180 的判据函数指针表，
//! 每条带稳定 ID 与归属域，注册顺序即运行顺序，一次命令全跑）；②失败
//! 隔离运行器（单条判据红不阻断后续——全量跑完才汇总，红项逐条入
//! 明细账，杜绝「一条红挡全绿视野」）；③全绿门禁（运行数 ≡ 注册数、
//! 红项数恒 0、ID 无重复三账齐才放行）。

use alloc::format;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const KTESTB09_VERSION: &str = "180-ktestb09-v1";

/// 段基（0x6B70）。
pub const SEG_BASE: u16 = 0x6B70;

/// 判据函数统一签名。
pub type CheckFn = fn() -> Result<(), u16>;

/// 断言条目：稳定 ID + 归属域 + 判据函数。
#[derive(Clone)]
pub struct AssertEntry {
    /// 稳定 ID（ktest 检索键，"UNX-F0xxx-J1" 形）。
    pub id: &'static str,
    /// 归属域（B09 批内分组）。
    pub domain: &'static str,
    /// 判据函数。
    pub check: CheckFn,
}

/// 单条运行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub id: &'static str,
    /// 绿/红。
    pub passed: bool,
    /// 红项错误码（绿为 0）。
    pub err: u16,
}

// ---------------------------------------------------------------------------
// B09 批注册表
// ---------------------------------------------------------------------------

/// B09 批全部断言（注册序 = 运行序 = 批内任务号升序）。
pub fn b09_registry() -> Vec<AssertEntry> {
    vec![
        AssertEntry {
            id: "UNX-F0166-J1",
            domain: "efi-watchdog",
            check: super::unx0166_watchdog::self_audit,
        },
        AssertEntry {
            id: "UNX-F0167-J1",
            domain: "efi-mixedmode",
            check: super::unx0167_mixedmode::self_audit,
        },
        AssertEntry {
            id: "UNX-F0168-J1",
            domain: "efi-ebscb",
            check: super::unx0168_ebscb::self_audit,
        },
        AssertEntry {
            id: "UNX-F0169-J1",
            domain: "efi-rtsvc",
            check: super::unx0169_rtsvc::self_audit,
        },
        AssertEntry {
            id: "UNX-F0170-J1",
            domain: "efi-sbchain",
            check: super::unx0170_sbchain::self_audit,
        },
        AssertEntry {
            id: "UNX-F0172-J1",
            domain: "efi-partdisc",
            check: super::unx172_partdisc::self_audit,
        },
        AssertEntry {
            id: "UNX-F0173-J1",
            domain: "efi-bootenum",
            check: super::unx0173_bootenum::self_audit,
        },
        AssertEntry {
            id: "UNX-F0174-J1",
            domain: "efi-reentry",
            check: super::unx0174_reentry::self_audit,
        },
        AssertEntry {
            id: "UNX-F0175-J1",
            domain: "efi-bsgc",
            check: super::unx0175_bsgc::self_audit,
        },
        AssertEntry {
            id: "UNX-F0176-J1",
            domain: "efi-svcdegrade",
            check: super::unx0176_svcdegrade::self_audit,
        },
        AssertEntry {
            id: "UNX-F0177-J1",
            domain: "efi-rtpnx",
            check: super::unx0177_rtpnx::self_audit,
        },
        AssertEntry {
            id: "UNX-F0178-J1",
            domain: "efi-envfp",
            check: super::unx178_envfp::self_audit,
        },
        AssertEntry {
            id: "UNX-F0179-J1",
            domain: "efi-faillog",
            check: super::unx179_faillog::self_audit,
        },
        AssertEntry {
            id: "UNX-F0180-J1",
            domain: "ktest-b09",
            check: self_audit,
        },
    ]
}

// ---------------------------------------------------------------------------
// 隔离运行器
// ---------------------------------------------------------------------------

/// B09 批运行器。
pub struct KtestB09Runner {
    registry: Vec<AssertEntry>,
    results: Vec<RunResult>,
    /// 运行是否全量完成（失败隔离红线：中途放弃即违例）。
    completed: bool,
}

impl KtestB09Runner {
    /// 以标准注册表建runner。
    pub fn boot() -> Self {
        KtestB09Runner {
            registry: b09_registry(),
            results: Vec::new(),
            completed: false,
        }
    }

    /// 注册条数。
    pub fn registered(&self) -> usize {
        self.registry.len()
    }

    /// 全量运行（失败隔离：单条红不阻断，全跑完才汇总）。
    pub fn run_all(&mut self) -> Vec<RunResult> {
        self.results.clear();
        for e in self.registry.iter() {
            let r = match (e.check)() {
                Ok(()) => RunResult {
                    id: e.id,
                    passed: true,
                    err: 0,
                },
                Err(code) => RunResult {
                    id: e.id,
                    passed: false,
                    err: code,
                },
            };
            self.results.push(r);
        }
        self.completed = true;
        self.results.clone()
    }

    /// 全绿门禁（判据主体）：全量完成 + 运行数 ≡ 注册数 + 红项恒 0。
    pub fn green_gate(&self) -> Result<(), u16> {
        if !self.completed {
            return Err(E_NOT_RUN);
        }
        if self.results.len() != self.registry.len() {
            return Err(E_RUN_SHORT);
        }
        for r in self.results.iter() {
            if !r.passed {
                return Err(E_NOT_ALL_GREEN);
            }
        }
        Ok(())
    }

    /// 红项明细（失败路径诊断用——标签(码)名单）。
    pub fn red_detail(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| alloc::format!("{}({:#06x})", r.id, r.err))
            .collect()
    }

    /// ID 唯一性审计（注册表红线）。
    pub fn id_unique(&self) -> Result<(), u16> {
        for i in 0..self.registry.len() {
            for j in (i + 1)..self.registry.len() {
                if self.registry[i].id == self.registry[j].id {
                    return Err(E_DUP_ID);
                }
            }
        }
        Ok(())
    }

    /// 域分组覆盖审计：B09 批 14 项须覆盖全部 14 个归属域。
    pub fn domain_coverage(&self) -> Result<usize, u16> {
        let mut seen: Vec<&'static str> = Vec::new();
        for e in self.registry.iter() {
            if !seen.contains(&e.domain) {
                seen.push(e.domain);
            }
        }
        if seen.len() != self.registry.len() {
            return Err(E_DOMAIN_GAPS);
        }
        Ok(seen.len())
    }

    /// 结果快照。
    pub fn snapshot(&self) -> (usize, usize, bool) {
        (
            self.results.len(),
            self.results.iter().filter(|r| r.passed).count(),
            self.completed,
        )
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x6B70 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum KtestB09Err {
    /// 未运行即求门禁。
    NotRun = SEG_BASE + 1,
    /// 运行数短于注册数。
    RunShort = SEG_BASE + 2,
    /// 有红项（非全绿）。
    NotAllGreen = SEG_BASE + 3,
    /// 断言 ID 重复。
    DupId = SEG_BASE + 4,
    /// 域覆盖缺口。
    DomainGaps = SEG_BASE + 5,
}

impl KtestB09Err {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtestB09Err::NotRun => "run_all 未执行即求全绿门禁",
            KtestB09Err::RunShort => "运行结果数 ≠ 注册条数",
            KtestB09Err::NotAllGreen => "存在红项，非全绿",
            KtestB09Err::DupId => "断言 ID 重复",
            KtestB09Err::DomainGaps => "归属域覆盖有缺口",
        }
    }
}

const E_NOT_RUN: u16 = KtestB09Err::NotRun.code();
const E_RUN_SHORT: u16 = KtestB09Err::RunShort.code();
const E_NOT_ALL_GREEN: u16 = KtestB09Err::NotAllGreen.code();
const E_DUP_ID: u16 = KtestB09Err::DupId.code();
const E_DOMAIN_GAPS: u16 = KtestB09Err::DomainGaps.code();

// ---------------------------------------------------------------------------
// 本体自证判据
// ---------------------------------------------------------------------------

/// 本体自证（注册进批表的 UNX-F0180-J1）：聚合器自身结构红线。
/// 注意：只查结构、不跑批——run_all 若在本体内递归调用会爆栈，
/// 全量运行验证走独立的 b09_full_green（ktest 外部入口）。
pub fn self_audit() -> Result<(), u16> {
    let r = KtestB09Runner::boot();
    // 一、注册表红线：ID 唯一、域全覆盖、注册数恰 14。
    r.id_unique()?;
    let domains = r.domain_coverage()?;
    if domains != 14 || r.registered() != 14 {
        return Err(E_DOMAIN_GAPS);
    }
    // 二、判别式段位对拍。
    if E_DOMAIN_GAPS != SEG_BASE + 5 || E_NOT_RUN != SEG_BASE + 1 {
        return Err(E_DOMAIN_GAPS);
    }
    Ok(())
}

/// J1 主体（ktest 一次命令全绿的外部入口）：全量运行 + 全绿门禁 +
/// 未跑拒门禁反证 + 红项明细空。不注册进批表（防自递归）。
pub fn b09_full_green() -> Result<(), u16> {
    let mut r = KtestB09Runner::boot();
    // 一、未跑先求门禁必须被拒（防「假全绿」）。
    if r.green_gate().is_ok() {
        return Err(E_NOT_RUN);
    }
    // 二、一次命令全跑、全绿、账平。
    let results = r.run_all();
    let (n, green, done) = r.snapshot();
    if !done || n != 14 || green != 14 || results.len() != 14 {
        return Err(E_RUN_SHORT);
    }
    r.green_gate()?;
    // 三、红项明细在全绿时恒空。
    if !r.red_detail().is_empty() {
        return Err(E_NOT_ALL_GREEN);
    }
    let _ = results;
    Ok(())
}
