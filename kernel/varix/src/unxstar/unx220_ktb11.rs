//! UNX-F0220 · ktest 引导面 B11 批断言集
//!
//! 锚点判据（施工书原文）：UNX-F0220-J1 本批判据聚合入 ktest，
//! 一次命令全绿。
//!
//! 设计：与 unx180（B09 批）同构——注册表 {ID, 名称, 判据入口} +
//! run_all 一次全跑 + green_gate（未跑拒、不全绿拒）+ 红项明细。
//! B11 批域：演练与生存面（F0226~F0233 八模块）。注意：
//! self_audit 只做结构自证，run_all 递归入本体即爆栈——全绿
//! 验证走独立外部入口 b11_full_green。
//!
//! 错误码段：0x2CC0（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0220_VERSION: &str = "F0220-ktb11-v1";

/// 诊断码段基址（UNX-F0220 独占）。
pub const SEG_BASE: u16 = 0x2CC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 未跑先求门禁。
    NotRun = SEG_BASE + 1,
    /// 运行数不足（注册数 ≠ 实跑数）。
    RunShort = SEG_BASE + 2,
    /// 非全绿。
    NotAllGreen = SEG_BASE + 3,
    /// 注册域有缺口。
    DomainGaps = SEG_BASE + 4,
    /// ID 重复。
    DupId = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 批内单条结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// 任务号。
    pub id: &'static str,
    /// 绿/红。
    pub green: bool,
    /// 红因（绿时空）。
    pub red: u16,
}

/// B11 批注册表（演练与生存面八件）。
struct Case {
    id: &'static str,
    name: &'static str,
    check: fn() -> Result<(), u16>,
}

/// 注册表真源（八件，ID 与域任务号一一对应）。
const REGISTRY: [Case; 8] = [
    Case {
        id: "UNX-F0226",
        name: "坏镜像拒收演练",
        check: super::unx0226_badimg::self_audit,
    },
    Case {
        id: "UNX-F0227",
        name: "只读侧断电演练",
        check: super::unx0227_powcut::self_audit,
    },
    Case {
        id: "UNX-F0228",
        name: "CSM/Legacy 降级链",
        check: super::unx0228_csmdetect::self_audit,
    },
    Case {
        id: "UNX-F0229",
        name: "SecureBoot 引导路径",
        check: super::unx0229_sbpath::self_audit,
    },
    Case {
        id: "UNX-F0230",
        name: "多盘引导冲突消解",
        check: super::unx0230_mediapick::self_audit,
    },
    Case {
        id: "UNX-F0231",
        name: "镜像降级回滚",
        check: super::unx0231_abrollback::self_audit,
    },
    Case {
        id: "UNX-F0232",
        name: "引导成功率统计账",
        check: super::unx0232_bootrate::self_audit,
    },
    Case {
        id: "UNX-F0233",
        name: "A1 域完工判据集",
        check: super::unx0233_a1final::self_audit,
    },
];

/// B11 批运行器。
#[derive(Debug, Default)]
pub struct KtestB11Runner {
    /// 结果流。
    results: Vec<CaseResult>,
    /// 已跑。
    done: bool,
}

impl KtestB11Runner {
    /// 装载注册表。
    pub fn boot() -> Self {
        KtestB11Runner {
            results: Vec::new(),
            done: false,
        }
    }

    /// 注册数。
    pub fn registered(&self) -> usize {
        REGISTRY.len()
    }

    /// ID 唯一红线。
    pub fn id_unique(&self) -> Result<(), u16> {
        for i in 0..REGISTRY.len() {
            for j in (i + 1)..REGISTRY.len() {
                if REGISTRY[i].id == REGISTRY[j].id {
                    return Err(E::DupId.code());
                }
            }
        }
        Ok(())
    }

    /// 一次命令全跑。
    pub fn run_all(&mut self) -> Vec<CaseResult> {
        self.results.clear();
        for c in REGISTRY.iter() {
            let green = (c.check)().is_ok();
            self.results.push(CaseResult {
                id: c.id,
                green,
                red: if green { 0 } else { E::NotAllGreen.code() },
            });
        }
        self.done = true;
        self.results.clone()
    }

    /// 快照：(注册数, 绿数, 已跑)。
    pub fn snapshot(&self) -> (usize, usize, bool) {
        (
            REGISTRY.len(),
            self.results.iter().filter(|r| r.green).count(),
            self.done,
        )
    }

    /// 门禁：已跑且全绿才放行。
    pub fn green_gate(&self) -> Result<(), u16> {
        if !self.done {
            return Err(E::NotRun.code());
        }
        if self.results.len() != REGISTRY.len() {
            return Err(E::RunShort.code());
        }
        if !self.results.iter().all(|r| r.green) {
            return Err(E::NotAllGreen.code());
        }
        Ok(())
    }

    /// 红项明细（全绿恒空）。
    pub fn red_detail(&self) -> Vec<CaseResult> {
        self.results.iter().filter(|r| !r.green).cloned().collect()
    }
}

/// 结构自证（注册进批表的 UNX-F0220-J1 外圈）：不跑批，防自递归。
pub fn self_audit() -> Result<(), u16> {
    let r = KtestB11Runner::boot();
    r.id_unique()?;
    if r.registered() != 8 {
        return Err(E::DomainGaps.code());
    }
    if E::NotRun.code() != SEG_BASE + 1 || E::DupId.code() != SEG_BASE + 5 {
        return Err(E::DomainGaps.code());
    }
    Ok(())
}

/// J1 主体（ktest 一次命令全绿外部入口）。
pub fn b11_full_green() -> Result<(), u16> {
    let mut r = KtestB11Runner::boot();
    // 未跑先求门禁必拒。
    if r.green_gate().is_ok() {
        return Err(E::NotRun.code());
    }
    let results = r.run_all();
    let (n, green, done) = r.snapshot();
    if !done || n != 8 || green != 8 || results.len() != 8 {
        return Err(E::RunShort.code());
    }
    r.green_gate()?;
    if !r.red_detail().is_empty() {
        return Err(E::NotAllGreen.code());
    }
    Ok(())
}
