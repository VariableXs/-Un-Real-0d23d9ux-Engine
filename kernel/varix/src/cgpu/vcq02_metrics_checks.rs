//! CGPU-F2562 判据层：可靠性模型与指标（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2562`
//!
//! **锚点判据（五指标/口径复用/目标表/两组）→ 判据族**：
//! METRIC 2 / CONV 2 / TABLE 3 / CALC 2 / META 4 = 13 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 五指标与定义与单位**判据侧独立写死字面量对拍**；可用性恰边界与常规
//! 值手算对拍（99h/60min → 9900 万分比）；目标表校验双向（合法放行/
//! 非法逐码拒）；版本化不可变语义独立验证（bump 后旧表逐字段原样）；
//! 码段判据 `!=` 防自判死（0x50..0x57 全排除）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcq02_metrics::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的五指标定义（单位口径逐字）。
const EXP_DEFS: [(MetricKind, &str, &str); 5] = [
    (MetricKind::Mtbf, "小时", "两次故障间的平均运行时长"),
    (MetricKind::Mttr, "分钟", "从故障到恢复的平均耗时"),
    (MetricKind::Availability, "万分比", "可服务时间占总时间之比"),
    (MetricKind::Rpo, "秒", "故障时允许丢失的最大数据时长"),
    (MetricKind::Rto, "秒", "故障后恢复服务的最大允许时长"),
];

/// 判据侧独立写死的口径复用声明。
const EXP_CONVENTION: &str = "口径复用——与遥测注册口径一致，不另立口径";

/// 判据侧独立写死的 V1 目标表（版本/条目逐字）。
const EXP_V1: (u32, [&str; 3], [u32; 3], [u32; 3]) =
    (1, ["显存管理", "显示输出", "渲染管线"], [1, 2, 1], [0, 0, 0]);

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [QmCode; 5] = [
    QmCode::METRIC_OUT_OF_TABLE,
    QmCode::TARGET_INVALID,
    QmCode::TABLE_VERSION_STALE,
    QmCode::UNIT_CONVENTION_DRIFT,
    QmCode::ENTRY_DUPLICATE,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 13;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vcq02 域自检入口。
pub fn run_vcq02_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-relmetrics");

    // ================= 一、五指标闭集（METRIC） =================

    // MET-1：五指标闭集判据侧独立点名 + 标签非空两两互异。
    let l: Vec<String> = MetricKind::ALL.iter().map(|m| m.label()).collect();
    let mut met1 = MetricKind::ALL.len() == METRIC_COUNT && METRIC_COUNT == 5;
    for i in 0..l.len() {
        if l[i].is_empty() {
            met1 = false;
        }
        for j in 0..l.len() {
            if i != j && l[i] == l[j] {
                met1 = false;
            }
        }
    }
    s.add("Q02-MET-五指标闭集标签互异", met1, "");

    // MET-2：五条定义逐字对拍（判据侧写死——单位与定义都不可漂移）。
    let mut met2 = METRIC_DEFS.len() == 5;
    for i in 0..5 {
        if METRIC_DEFS[i].kind != EXP_DEFS[i].0
            || METRIC_DEFS[i].unit != EXP_DEFS[i].1
            || METRIC_DEFS[i].definition != EXP_DEFS[i].2
        {
            met2 = false;
        }
    }
    s.add("Q02-MET-五定义字面量对拍", met2, "");

    // ================= 二、口径复用（CONV） =================

    // CV-1：口径复用声明逐字对拍。
    s.add("Q02-CV-口径复用声明对拍", UNIT_CONVENTION_NOTE == EXP_CONVENTION, "");

    // CV-2：可用性万分比口径独立重算——99h/60min → 9900（手算：
    // mttr=1h，99/(99+1)=0.99 → 9900 万分比）。
    s.add(
        "Q02-CV-万分比口径独立重算",
        availability_bp(99.0, 60.0) == 9900,
        "",
    );

    // ================= 三、目标表（TABLE） =================

    // TAB-1：默认表 V1 字面量对拍（版本 1 + 三条目逐字 + 升序确定性）。
    let mut tab1 = TARGET_TABLE_V1_VERSION == EXP_V1.0 && TARGET_TABLE_V1_ENTRIES.len() == 3;
    for i in 0..3 {
        if TARGET_TABLE_V1_ENTRIES[i].subsystem != EXP_V1.1[i]
            || TARGET_TABLE_V1_ENTRIES[i].rto_s != EXP_V1.2[i]
            || TARGET_TABLE_V1_ENTRIES[i].rpo_s != EXP_V1.3[i]
        {
            tab1 = false;
        }
    }
    tab1 = tab1
        && TARGET_TABLE_V1_ENTRIES[0].subsystem < TARGET_TABLE_V1_ENTRIES[1].subsystem
        && TARGET_TABLE_V1_ENTRIES[1].subsystem < TARGET_TABLE_V1_ENTRIES[2].subsystem;
    s.add("Q02-TAB-V1目标表字面量对拍", tab1, "");

    // TAB-2：校验双向——合法放行；RPO>RTO 与 RTO=0 与乱序/重复逐码拒。
    let ok = TargetTable {
        version: 1,
        entries: vec![
            RelTarget { subsystem: "乙", rto_s: 3, rpo_s: 0 },
            RelTarget { subsystem: "甲", rto_s: 2, rpo_s: 1 },
        ],
    };
    let bad_rpo = TargetTable {
        version: 1,
        entries: vec![RelTarget { subsystem: "甲", rto_s: 1, rpo_s: 2 }],
    };
    let bad_rto = TargetTable {
        version: 1,
        entries: vec![RelTarget { subsystem: "甲", rto_s: 0, rpo_s: 0 }],
    };
    let bad_dup = TargetTable {
        version: 1,
        entries: vec![
            RelTarget { subsystem: "甲", rto_s: 1, rpo_s: 0 },
            RelTarget { subsystem: "甲", rto_s: 2, rpo_s: 0 },
        ],
    };
    s.add(
        "Q02-TAB-校验双向逐码拒",
        validate_table(&ok) == Ok(())
            && validate_table(&bad_rpo) == Err(QmCode::TARGET_INVALID)
            && validate_table(&bad_rto) == Err(QmCode::TARGET_INVALID)
            && validate_table(&bad_dup) == Err(QmCode::ENTRY_DUPLICATE),
        "",
    );

    // TAB-3：版本化不可变语义——bump 产新表（版本 +1、升序确定性），
    // 旧表逐字段原样（发布后不可变）。
    let base = TargetTable {
        version: 1,
        entries: vec![RelTarget { subsystem: "甲", rto_s: 2, rpo_s: 0 }],
    };
    let rev = publish_revision(
        &base,
        vec![
            RelTarget { subsystem: "丙", rto_s: 2, rpo_s: 0 },
            RelTarget { subsystem: "乙", rto_s: 4, rpo_s: 1 },
        ],
    );
    let tab3 = match rev {
        Ok(t) => {
            t.version == 2
                && t.entries.len() == 2
                && t.entries[0].subsystem == "丙"
                && t.entries[1].subsystem == "乙"
                && base.version == 1
                && base.entries.len() == 1
                && base.entries[0].subsystem == "甲"
                && base.entries[0].rto_s == 2
        }
        _ => false,
    };
    s.add("Q02-TAB-版本化不可变语义", tab3, "");

    // ================= 四、可用性计算（CALC） =================

    // CAC-1：恰边界双向——MTTR=0 → 满万分比；MTBF=0 → 0；双零 → 0（诚实退化）。
    s.add(
        "Q02-CAC-恰边界双向",
        availability_bp(100.0, 0.0) == 10000
            && availability_bp(0.0, 10.0) == 0
            && availability_bp(0.0, 0.0) == 0,
        "",
    );

    // CAC-2：常规值手算对拍（97h/180min=3h → 97/100 → 9700）。
    s.add("Q02-CAC-常规值手算对拍", availability_bp(97.0, 180.0) == 9700, "");

    // ================= 五、判据自检（META） =================

    // META-2：判据容量无截断。
    s.add("Q02-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x58xx，且 != 0x50..0x57（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x58)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
                && hi != 0x54 && hi != 0x55 && hi != 0x56 && hi != 0x57
        });
    s.add("Q02-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("Q02-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("Q02-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
