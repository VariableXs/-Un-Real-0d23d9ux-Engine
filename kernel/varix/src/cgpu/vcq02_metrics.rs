//! CGPU-F2562 · 可靠性模型与指标（CGPU-Q 域 · Q02 批次开工 · 可靠性模型主题）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2562`
//!
//! 可靠性模型：可靠性指标（MTBF/MTTR/可用性/恢复点目标 RPO/恢复时间目
//! 标 RTO——五指标——指标定义（口径复用）；各子系统 RTO/RPO 目标表——
//! 目标表版本化；测试（指标/目标两组）。
//!
//! ## 要点一：五指标闭集
//!
//! MTBF（平均无故障时间）/MTTR（平均恢复时间）/可用性/RPO（恢复点目
//! 标——最多丢多少数据）/RTO（恢复时间目标——最多停多久）——官方五指
//! 标封闭枚举，表外不立指标；每指标一条定义与单位口径（字面量冻结，
//! 判据独立对拍）。
//!
//! ## 要点二：口径复用
//!
//! 单位口径与遥测注册口径一致，不另立口径——口径分叉则指标不可比，
//! 不可比指标等于没有指标。
//!
//! ## 要点三：目标表版本化
//!
//! 各子系统 RTO/RPO 目标表带版本号：发布后不可变（改目标=发新版本，
//! 旧版本留档可追溯）；子系统条目按名称升序（确定性）；校验规则
//! （RTO>0、RPO≤RTO）违反即专属码拒。
//!
//! ## 要点四：可用性是算出来的
//!
//! 可用性 = MTBF/(MTBF+MTTR)，万分比整数口径；恰边界（MTTR=0 → 满
//! 分，MTBF=0 → 0）与常规值全部判据侧手算对拍。
//!
//! ## 要点五：零 panic 面 + 诊断码独占 0x58xx 段
//!
//! 与 vcq01（0x56xx）/cgr01（0x57xx）/vco01（0x55xx）等互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、五指标闭集与定义（口径复用）
// ---------------------------------------------------------------------------

/// 可靠性五指标（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    /// 平均无故障时间（MTBF）。
    Mtbf,
    /// 平均恢复时间（MTTR）。
    Mttr,
    /// 可用性。
    Availability,
    /// 恢复点目标（RPO——最多丢多少数据）。
    Rpo,
    /// 恢复时间目标（RTO——最多停多久）。
    Rto,
}

/// 指标总数。
pub const METRIC_COUNT: usize = 5;

impl MetricKind {
    /// 全部指标（官方序）。
    pub const ALL: [MetricKind; METRIC_COUNT] = [
        MetricKind::Mtbf,
        MetricKind::Mttr,
        MetricKind::Availability,
        MetricKind::Rpo,
        MetricKind::Rto,
    ];

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            MetricKind::Mtbf => "平均无故障时间".to_string(),
            MetricKind::Mttr => "平均恢复时间".to_string(),
            MetricKind::Availability => "可用性".to_string(),
            MetricKind::Rpo => "恢复点目标".to_string(),
            MetricKind::Rto => "恢复时间目标".to_string(),
        }
    }
}

/// 指标定义（定义与单位口径——字面量冻结，判据独立对拍）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricDef {
    /// 指标。
    pub kind: MetricKind,
    /// 单位口径。
    pub unit: &'static str,
    /// 定义（人话）。
    pub definition: &'static str,
}

/// 五指标定义表（口径复用——与遥测注册口径一致）。
pub const METRIC_DEFS: [MetricDef; 5] = [
    MetricDef { kind: MetricKind::Mtbf, unit: "小时", definition: "两次故障间的平均运行时长" },
    MetricDef { kind: MetricKind::Mttr, unit: "分钟", definition: "从故障到恢复的平均耗时" },
    MetricDef { kind: MetricKind::Availability, unit: "万分比", definition: "可服务时间占总时间之比" },
    MetricDef { kind: MetricKind::Rpo, unit: "秒", definition: "故障时允许丢失的最大数据时长" },
    MetricDef { kind: MetricKind::Rto, unit: "秒", definition: "故障后恢复服务的最大允许时长" },
];

/// 口径复用声明（锚点「口径复用」——判据逐字对拍）。
pub const UNIT_CONVENTION_NOTE: &str = "口径复用——与遥测注册口径一致，不另立口径";

// ---------------------------------------------------------------------------
// 二、可用性计算（万分比整数口径）
// ---------------------------------------------------------------------------

/// 可用性 = MTBF/(MTBF+MTTR)，万分比整数口径。
///
/// mtbf_h 单位小时（f32 纯算术）；mttr_min 单位分钟。恰边界：
/// mttr=0 → 10000（满）；mtbf=0 → 0（全挂）。分母零（双零）按 0 处理
/// （无运行即无可用——诚实退化不 panic）。
pub fn availability_bp(mtbf_h: f32, mttr_min: f32) -> u32 {
    let mttr_h = mttr_min / 60.0;
    let total = mtbf_h + mttr_h;
    if total <= 0.0 {
        return 0;
    }
    let bp = (mtbf_h / total) * 10000.0;
    if bp >= 10000.0 {
        10000
    } else {
        bp as u32
    }
}

// ---------------------------------------------------------------------------
// 三、目标表版本化（各子系统 RTO/RPO）
// ---------------------------------------------------------------------------

/// 单子系统可靠性目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelTarget {
    /// 子系统名（表内按此升序——确定性）。
    pub subsystem: &'static str,
    /// 恢复时间目标（秒，必须 >0）。
    pub rto_s: u32,
    /// 恢复点目标（秒，必须 ≤ rto_s）。
    pub rpo_s: u32,
}

/// 版本化目标表（发布后不可变——改目标=发新版本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetTable {
    /// 版本号（单调递增）。
    pub version: u32,
    /// 条目（按 subsystem 升序）。
    pub entries: Vec<RelTarget>,
}

/// 目标表校验：RTO>0、RPO≤RTO、子系统名非空且不重复、升序确定性。
pub fn validate_table(t: &TargetTable) -> Result<(), QmCode> {
    if t.version == 0 {
        return Err(QmCode::TABLE_VERSION_STALE);
    }
    let mut i = 0;
    while i < t.entries.len() {
        let e = &t.entries[i];
        if e.rto_s == 0 {
            return Err(QmCode::TARGET_INVALID);
        }
        if e.rpo_s > e.rto_s {
            return Err(QmCode::TARGET_INVALID);
        }
        if e.subsystem.is_empty() {
            return Err(QmCode::TARGET_INVALID);
        }
        if i > 0 {
            let prev = &t.entries[i - 1];
            if prev.subsystem >= e.subsystem {
                // 乱序或重复（重复=同名条目，均拒绝——确定性纪律）
                return Err(QmCode::ENTRY_DUPLICATE);
            }
        }
        i += 1;
    }
    Ok(())
}

/// 版本化发布：在旧表基础上产出修订新表（版本 +1；旧表原样留档——
/// 调用方持有旧表引用不受影响，不可变语义由值语义保证）。
///
/// 新表条目按 subsystem 升序重排（确定性），校验不过则整表拒绝。
pub fn publish_revision(base: &TargetTable, revised: Vec<RelTarget>) -> Result<TargetTable, QmCode> {
    let mut sorted = revised;
    // 插入排序（条目少，无 panic 面）
    for i in 1..sorted.len() {
        let mut j = i;
        while j > 0 && sorted[j - 1].subsystem > sorted[j].subsystem {
            sorted.swap(j - 1, j);
            j -= 1;
        }
    }
    let candidate = TargetTable { version: base.version + 1, entries: sorted };
    validate_table(&candidate)?;
    Ok(candidate)
}

/// 默认目标表 V1（渲染管线/显示输出/显存管理——字面量钉死，判据对拍）。
pub const TARGET_TABLE_V1_VERSION: u32 = 1;
/// V1 条目（字面量钉死）。
pub const TARGET_TABLE_V1_ENTRIES: [RelTarget; 3] = [
    RelTarget { subsystem: "显存管理", rto_s: 1, rpo_s: 0 },
    RelTarget { subsystem: "显示输出", rto_s: 2, rpo_s: 0 },
    RelTarget { subsystem: "渲染管线", rto_s: 1, rpo_s: 0 },
];

// ---------------------------------------------------------------------------
// 四、错误契约（独占 0x58xx 段）
// ---------------------------------------------------------------------------

/// vcq02 诊断码。独占 `0x58xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QmCode(pub u16);

impl QmCode {
    /// 指标越界（五指标表外）。
    pub const METRIC_OUT_OF_TABLE: QmCode = QmCode(0x5801);
    /// 目标条目非法（RTO=0/RPO>RTO/空名）。
    pub const TARGET_INVALID: QmCode = QmCode(0x5802);
    /// 目标表版本非法（0 版/回退版）。
    pub const TABLE_VERSION_STALE: QmCode = QmCode(0x5803);
    /// 口径漂移（单位与冻结口径不符）。
    pub const UNIT_CONVENTION_DRIFT: QmCode = QmCode(0x5804);
    /// 条目重复或乱序（确定性违约）。
    pub const ENTRY_DUPLICATE: QmCode = QmCode(0x5805);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            QmCode::METRIC_OUT_OF_TABLE => "可靠性指标越界：五指标表外不得立指标".into(),
            QmCode::TARGET_INVALID => "目标条目非法：RTO 必须为正且 RPO ≤ RTO".into(),
            QmCode::TABLE_VERSION_STALE => "目标表版本非法：版本必须为正且单调递增".into(),
            QmCode::UNIT_CONVENTION_DRIFT => "口径漂移：单位与遥测注册口径不符".into(),
            QmCode::ENTRY_DUPLICATE => "条目重复或乱序：子系统名必须升序且唯一".into(),
            QmCode(_) => "未知 vcq02 可靠性指标域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑（回归两组：指标/目标）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 五指标闭集与定义齐() {
        assert_eq!(MetricKind::ALL.len(), 5);
        assert_eq!(METRIC_DEFS.len(), 5);
        for i in 0..5 {
            assert_eq!(METRIC_DEFS[i].kind, MetricKind::ALL[i]);
        }
    }

    #[test]
    fn 可用性恰边界与常规值() {
        assert_eq!(availability_bp(100.0, 0.0), 10000);
        assert_eq!(availability_bp(0.0, 10.0), 0);
        assert_eq!(availability_bp(99.0, 60.0), 9900);
    }

    #[test]
    fn 目标表校验双向() {
        let ok = TargetTable {
            version: 1,
            entries: vec![
                RelTarget { subsystem: "乙", rto_s: 3, rpo_s: 0 },
                RelTarget { subsystem: "甲", rto_s: 2, rpo_s: 1 },
            ],
        };
        assert_eq!(validate_table(&ok), Ok(()));
        let bad = TargetTable {
            version: 1,
            entries: vec![RelTarget { subsystem: "甲", rto_s: 1, rpo_s: 2 }],
        };
        assert_eq!(validate_table(&bad), Err(QmCode::TARGET_INVALID));
    }

    #[test]
    fn 版本化发布旧表不变() {
        let base = TargetTable {
            version: 1,
            entries: vec![RelTarget { subsystem: "甲", rto_s: 2, rpo_s: 0 }],
        };
        let rev = publish_revision(&base, vec![
            RelTarget { subsystem: "乙", rto_s: 4, rpo_s: 1 },
            RelTarget { subsystem: "丙", rto_s: 2, rpo_s: 0 },
        ])
        .unwrap();
        assert_eq!(rev.version, 2);
        assert_eq!(rev.entries[0].subsystem, "丙");
        assert_eq!(base.version, 1);
        assert_eq!(base.entries.len(), 1);
    }
}
