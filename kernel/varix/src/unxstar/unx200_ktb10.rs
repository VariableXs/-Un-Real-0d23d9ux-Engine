//! UNX-F0200 · ktest 引导面 B10 批断言集
//!
//! 锚点判据（施工书原文）：UNX-F0200-J1 本批判据聚合入 ktest，
//! 一次命令全绿。
//!
//! 设计：与 unx180/unx220 同构——注册表 {ID, 名称, 判据入口} +
//! run_all 一次全跑 + green_gate（未跑拒、不全绿拒）+ 红项明细。
//! B10 批域：耗时账/段审计 + 报告域十三件（F0198~F0199、
//! F0207~F0219，共十五）。self_audit 只做结构自证，全绿验证
//! 走独立外部入口 b10_full_green（防自递归爆栈）。
//!
//! 错误码段：0x2BC0（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0200_VERSION: &str = "F0200-ktb10-v1";

/// 诊断码段基址（UNX-F0200 独占）。
pub const SEG_BASE: u16 = 0x2BC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 未跑先求门禁。
    NotRun = SEG_BASE + 1,
    /// 运行数不足。
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
    /// 红因（绿时 0）。
    pub red: u16,
}

/// 注册表项。
struct Case {
    id: &'static str,
    name: &'static str,
    check: fn() -> Result<(), u16>,
}

/// B10 批注册表真源（十五件）。
const REGISTRY: [Case; 15] = [
    Case {
        id: "UNX-F0198",
        name: "init 阶段耗时账",
        check: super::unx198_timeline::self_audit,
    },
    Case {
        id: "UNX-F0199",
        name: "早年数据段清零审计",
        check: super::unx199_bssaudit::self_audit,
    },
    Case {
        id: "UNX-F0207",
        name: "报告敏感信息脱敏",
        check: super::unx207_desens::self_audit,
    },
    Case {
        id: "UNX-F0208",
        name: "报告展示接口",
        check: super::unx208_reportui::self_audit,
    },
    Case {
        id: "UNX-F0209",
        name: "引导时间线甘特图",
        check: super::unx209_gantt::self_audit,
    },
    Case {
        id: "UNX-F0210",
        name: "硬件指纹链",
        check: super::unx210_hwfp::self_audit,
    },
    Case {
        id: "UNX-F0211",
        name: "多次引导聚合统计",
        check: super::unx211_bootstat::self_audit,
    },
    Case {
        id: "UNX-F0212",
        name: "回归对比",
        check: super::unx212_regdiff::self_audit,
    },
    Case {
        id: "UNX-F0213",
        name: "报告完整性签名",
        check: super::unx213_repsign::self_audit,
    },
    Case {
        id: "UNX-F0214",
        name: "体验事件埋点",
        check: super::unx214_frustr::self_audit,
    },
    Case {
        id: "UNX-F0215",
        name: "日志-报告联动",
        check: super::unx215_logtrace::self_audit,
    },
    Case {
        id: "UNX-F0216",
        name: "JSON 导出",
        check: super::unx216_jsonexp::self_audit,
    },
    Case {
        id: "UNX-F0217",
        name: "零静默探针网络",
        check: super::unx217_probezero::self_audit,
    },
    Case {
        id: "UNX-F0218",
        name: "消费方接口冻结",
        check: super::unx218_consfrozen::self_audit,
    },
    Case {
        id: "UNX-F0219",
        name: "报告生成性能预算",
        check: super::unx219_perfbudget::self_audit,
    },
];

/// B10 批运行器。
#[derive(Debug, Default)]
pub struct KtestB10Runner {
    /// 结果流。
    results: Vec<CaseResult>,
    /// 已跑。
    done: bool,
}

impl KtestB10Runner {
    /// 装载注册表。
    pub fn boot() -> Self {
        KtestB10Runner {
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

/// 结构自证（注册进批表的 UNX-F0200-J1 外圈）：不跑批，防自递归。
pub fn self_audit() -> Result<(), u16> {
    let r = KtestB10Runner::boot();
    r.id_unique()?;
    if r.registered() != 15 {
        return Err(E::DomainGaps.code());
    }
    if E::NotRun.code() != SEG_BASE + 1 || E::DupId.code() != SEG_BASE + 5 {
        return Err(E::DomainGaps.code());
    }
    Ok(())
}

/// J1 主体（ktest 一次命令全绿外部入口）。
pub fn b10_full_green() -> Result<(), u16> {
    let mut r = KtestB10Runner::boot();
    // 未跑先求门禁必拒。
    if r.green_gate().is_ok() {
        return Err(E::NotRun.code());
    }
    let results = r.run_all();
    let (n, green, done) = r.snapshot();
    if !done || n != 15 || green != 15 || results.len() != 15 {
        return Err(E::RunShort.code());
    }
    r.green_gate()?;
    if !r.red_detail().is_empty() {
        return Err(E::NotAllGreen.code());
    }
    Ok(())
}
