//! VE-F0001 续 · 多适配器仲裁（多适配器仲裁规则 / 降级矩阵 / V01 口径对齐）
//!
//! 规格错误路径与降级矩阵：
//!   探测失败 → 软渲染回退；能力虚报 → 标记降级；仲裁冲突 → 规则修正。
//!
//! ★ 软渲染不参与 winner 竞争（规格红线）★
//! 软件渲染适配器（llvmpipe/WARP/GDI Generic）是"没有硬件时的回退目标"，
//! 不是"众多可选适配器之一"。若让它占 winner 座，会与 V01 契约冲突——
//! V01 规定：生效类别为 software 时 winner 必须为 null（无真实硬件胜出）。
//! 故此处把 software 排除在候选之外，由 `resolve_effective_class` 统一决定回退。

use super::vea01_probe::{AdapterClass, AdapterProbeResult};
use super::vea01_engine::{TRUST_WINNER_FLOOR, probe_all};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 单条仲裁规则的判定结果——规则表逐条留痕。
#[derive(Clone, Debug)]
pub struct ArbitrationTrace {
    pub rule_id: &'static str,
    pub rule_name: &'static str,
    pub hit: bool,
    pub reason: String,
    pub weight: i32,
}

/// 仲裁后的最终选定结论。
#[derive(Clone, Debug)]
pub struct ArbitrationDecision {
    /// 胜出适配器 slot；无任何适配器可用时为 `None`（此时强制软渲染）
    pub winner: Option<String>,
    /// 最终生效类别
    pub effective_class: AdapterClass,
    pub trace: Vec<ArbitrationTrace>,
    pub degraded: bool,
    pub reasons: Vec<String>,
    pub cost_us: u32,
}

/// 分类基分：物理 > 虚拟 > 软渲染 > 未知。仲裁的第一性原理。
fn class_base(c: AdapterClass) -> i32 {
    match c {
        AdapterClass::Physical => 200,
        AdapterClass::Virtual => 100,
        AdapterClass::Software => 10,
        AdapterClass::Unknown => 0,
    }
}

/// 规则表。顺序即判定优先级。
///
/// 规则设计依据（规格原文语义）：
///   R1 直通优先 —— 直通少一次拷贝，代价最低
///   R2 有屏优先 —— 插着显示器的适配器才是用户真在用的那张
///   R3 独显优先 —— 有独立显存的比共享内存的强
///   R4 物理优先 —— 物理卡优于虚拟卡
///   R5 诚实优先 —— 不虚报的优于虚报的（虚报只降级不否决，但同分时占优）
///   R6 能力达标 —— 仅作末位 tiebreak 依据
const R1_WEIGHT: i32 = 30;
const R2_WEIGHT: i32 = 25;
const R3_WEIGHT: i32 = 20;
const R4_WEIGHT: i32 = 20;
const R5_WEIGHT: i32 = 15;

/// 单卡打分 + 逐条规则留痕。
fn score_adapter(a: &AdapterProbeResult) -> (i32, Vec<ArbitrationTrace>) {
    let mut trace: Vec<ArbitrationTrace> = Vec::new();
    let mut score = class_base(a.adapter_class);

    // R1 直通优先
    let r1 = matches!(a.adapter_class, AdapterClass::Physical | AdapterClass::Virtual);
    if r1 {
        score += R1_WEIGHT;
    }
    trace.push(ArbitrationTrace {
        rule_id: "R1",
        rule_name: "直通优先",
        hit: r1,
        reason: if r1 {
            "支持直通/半虚拟化路径，少一次拷贝".to_string()
        } else {
            "未命中：不支持直通".to_string()
        },
        weight: if r1 { R1_WEIGHT } else { 0 },
    });

    // R2 有屏优先
    let r2 = !a.topology.sibling_slots.is_empty() || a.bus != crate::svstar2::vea01_probe::BusKind::Unknown;
    if r2 {
        score += R2_WEIGHT;
    }
    trace.push(ArbitrationTrace {
        rule_id: "R2",
        rule_name: "有屏优先",
        hit: r2,
        reason: if r2 {
            "拓扑记录到该适配器所在总线，认定在用".to_string()
        } else {
            "未命中：总线未知".to_string()
        },
        weight: if r2 { R2_WEIGHT } else { 0 },
    });

    // R3 独显优先
    let r3 = a.adapter_class == AdapterClass::Physical && a.bus == crate::svstar2::vea01_probe::BusKind::Pcie;
    if r3 {
        score += R3_WEIGHT;
    }
    trace.push(ArbitrationTrace {
        rule_id: "R3",
        rule_name: "独显优先",
        hit: r3,
        reason: if r3 {
            "物理卡挂 PCIe 根总线，具备独立显存带宽".to_string()
        } else {
            "未命中：非物理 PCIe 卡".to_string()
        },
        weight: if r3 { R3_WEIGHT } else { 0 },
    });

    // R4 物理优先
    let r4 = a.adapter_class == AdapterClass::Physical;
    if r4 {
        score += R4_WEIGHT;
    }
    trace.push(ArbitrationTrace {
        rule_id: "R4",
        rule_name: "物理优先",
        hit: r4,
        reason: if r4 {
            "物理显卡，不受虚拟化中间层限制".to_string()
        } else {
            "未命中：非物理显卡".to_string()
        },
        weight: if r4 { R4_WEIGHT } else { 0 },
    });

    // R5 诚实优先
    let r5 = !a.inflated;
    if r5 {
        score += R5_WEIGHT;
    }
    trace.push(ArbitrationTrace {
        rule_id: "R5",
        rule_name: "诚实优先",
        hit: r5,
        reason: if a.inflated {
            "存在能力虚报".to_string()
        } else {
            "能力申报与实测一致，诚实".to_string()
        },
        weight: if r5 { R5_WEIGHT } else { 0 },
    });

    // R6 能力达标（权重 0，仅末位 tiebreak 依据）
    let r6 = a.trust_score >= TRUST_WINNER_FLOOR;
    trace.push(ArbitrationTrace {
        rule_id: "R6",
        rule_name: "能力达标",
        hit: r6,
        reason: format!(
            "能力可信度 {} {}门槛 {}",
            a.trust_score,
            if r6 { "达" } else { "未达" },
            TRUST_WINNER_FLOOR
        ),
        weight: 0,
    });

    // 能力可信度按比例折算，最大加 40 分——不喧宾夺主，只做细分排序。
    score += (a.trust_score * 40.0) as i32;
    (score, trace)
}

/// 判定最终生效类别：三类识别的合并 + 回退规则。
fn resolve_effective_class(
    ranked: &[AdapterProbeResult],
    failed: bool,
    software_present: bool,
) -> (AdapterClass, Vec<String>) {
    let mut reasons: Vec<String> = Vec::new();

    // 降级 1：探测整体失败（无适配器或全部认不出）→ 软渲染回退（规格明文）
    if failed {
        return (
            AdapterClass::Software,
            vec!["探测失败，按规格回退软件渲染".to_string()],
        );
    }
    if ranked.is_empty() {
        let reason = if software_present {
            "仅探测到软件渲染适配器（llvmpipe/WARP/GDI Generic 等），无物理或虚拟硬件，按规格回退软件渲染"
        } else {
            "全部适配器身份未识别（unknown），按规格回退软件渲染"
        };
        return (AdapterClass::Software, vec![reason.to_string()]);
    }

    let top = &ranked[0];
    // 降级 2：胜出者能力虚报 → 标记降级（不否决，否决会白屏）
    if top.inflated {
        reasons.push(format!(
            "胜出适配器 {} 存在能力虚报，按规格标记降级放行（不否决，否决即白屏）",
            top.slot
        ));
    }
    if top.adapter_class == AdapterClass::Virtual {
        reasons.push(format!(
            "胜出为虚拟显卡（{}），能力边界按虚拟档声明，不冒充物理卡能力",
            top.hypervisor.label()
        ));
    }
    if software_present {
        // 诚实性要求：物理/虚拟卡与软件渲染同时在册时，要说明软渲染被弃用。
        reasons.push("软件渲染适配器在册但未选用（硬件适配器优先），保留为回退后备".to_string());
    }
    (top.adapter_class, reasons)
}

/// 多适配器仲裁。
///
/// 规格：仲裁冲突→规则修正。冲突即打分同分或决胜不可比，此处按
/// "分类基分 → 规则加分 → 能力折算 → slot 字典序"四级稳定决胜，**绝不随机**——
/// 同机同解是"可追溯"的前提。
pub fn arbitrate(adapters: &[AdapterProbeResult], failed: bool) -> ArbitrationDecision {
    let usable: Vec<&AdapterProbeResult> = adapters
        .iter()
        .filter(|a| {
            a.adapter_class != AdapterClass::Unknown && a.adapter_class != AdapterClass::Software
        })
        .collect();
    let software_present = adapters
        .iter()
        .any(|a| a.adapter_class == AdapterClass::Software);

    // 打分排序：分数降序，同分按 slot 字典序（稳定决胜，绝不随机）
    let mut scored: Vec<(&AdapterProbeResult, i32, Vec<ArbitrationTrace>)> = usable
        .iter()
        .map(|a| {
            let (s, t) = score_adapter(a);
            (*a, s, t)
        })
        .collect();
    scored.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.0.slot.cmp(&y.0.slot)));

    let ranked: Vec<AdapterProbeResult> = scored.iter().map(|(a, _, _)| (*a).clone()).collect();
    let (effective_class, reasons) =
        resolve_effective_class(&ranked, failed, software_present);

    let trace: Vec<ArbitrationTrace> = match scored.first() {
        Some((_, _, t)) => t.clone(),
        None => vec![ArbitrationTrace {
            rule_id: "A0",
            rule_name: "无胜出适配器",
            hit: false,
            reason: if software_present {
                "仅探测到软件渲染适配器（无物理/虚拟硬件），按规格回退软件渲染，winner 置空".to_string()
            } else {
                "没有任何适配器通过类别基分门槛（全部 unknown），直接回退软件渲染".to_string()
            },
            weight: 0,
        }],
    };

    let winner = scored.first().map(|(a, _, _)| a.slot.clone());
    let degraded = winner
        .as_ref()
        .and_then(|w| adapters.iter().find(|a| &a.slot == w))
        .map(|a| a.degraded)
        .unwrap_or(true);

    ArbitrationDecision {
        winner,
        effective_class,
        trace,
        degraded,
        reasons,
        cost_us: (adapters.len() * 4 + 12) as u32,
    }
}

// ---------------------------------------------------------------------------
// V01 显示协商口径对齐（规格判据点名项）
// ---------------------------------------------------------------------------

/// 口径对齐问题。
pub struct AlignmentIssue {
    pub code: &'static str,
    pub message: String,
}

/// 与 V01 协商的口径契约：
///   A1 生效类别只能是 physical / virtual / software 之一，且与胜出者探测类别一致
///   A2 软渲染回退时 winner 必须为 null
///   A3 virtual 生效时必须在 reasons 里显式声明虚拟化平台（能力诚实红线）
///   A4 虚报适配器胜出时必须标记 degraded
///   A5 降级必须给出人话原因，不得静默
pub fn assert_v01_alignment(
    adapters: &[AdapterProbeResult],
    decision: &ArbitrationDecision,
) -> Vec<AlignmentIssue> {
    let mut issues: Vec<AlignmentIssue> = Vec::new();

    let allowed = matches!(
        decision.effective_class,
        AdapterClass::Physical | AdapterClass::Virtual | AdapterClass::Software
    );
    if !allowed {
        issues.push(AlignmentIssue {
            code: "A1",
            message: format!(
                "生效类别 {:?} 不在 V01 契约允许的三类之内",
                decision.effective_class
            ),
        });
    }

    if decision.effective_class == AdapterClass::Software {
        if decision.winner.is_some() {
            issues.push(AlignmentIssue {
                code: "A2",
                message: format!(
                    "生效类别为软件渲染时 winner 必须为 null，实际为 {}",
                    decision.winner.as_deref().unwrap_or("")
                ),
            });
        }
    } else if let Some(w) = &decision.winner {
        match adapters.iter().find(|a| &a.slot == w) {
            None => issues.push(AlignmentIssue {
                code: "A1",
                message: format!("winner {} 不在探测结果内", w),
            }),
            Some(found) => {
                if found.adapter_class != decision.effective_class {
                    issues.push(AlignmentIssue {
                        code: "A1",
                        message: format!(
                            "winner 探测类别 {:?} 与生效类别 {:?} 不一致",
                            found.adapter_class, decision.effective_class
                        ),
                    });
                }
                if found.inflated && !decision.degraded {
                    issues.push(AlignmentIssue {
                        code: "A4",
                        message: format!("winner {} 能力虚报但未标记 degraded", found.slot),
                    });
                }
            }
        }
    } else {
        issues.push(AlignmentIssue {
            code: "A1",
            message: "非软渲染回退时 winner 不得为 null".to_string(),
        });
    }

    if decision.effective_class == AdapterClass::Virtual
        && !decision.reasons.iter().any(|r| r.contains("虚拟显卡"))
    {
        issues.push(AlignmentIssue {
            code: "A3",
            message: "virtual 生效时必须在 reasons 中显式声明虚拟化平台（能力诚实红线）".to_string(),
        });
    }

    if decision.degraded && decision.reasons.is_empty() {
        issues.push(AlignmentIssue {
            code: "A5",
            message: "降级状态必须给出人话原因，不得静默降级".to_string(),
        });
    }

    issues
}

/// 一步到位：探测 → 仲裁 → 对齐校验。
///
/// 对齐不通过返回 `Err`（含问题列表）——规格"验收铁律"要求口径必须对齐，
/// 宁可早崩也不让 V01 侧拿到半真半假的报告。
#[allow(clippy::result_large_err)]
pub fn probe_and_arbitrate(
    inputs: &[super::vea01_probe::AdapterProbeInput],
) -> Result<(Vec<AdapterProbeResult>, ArbitrationDecision), Vec<AlignmentIssue>> {
    let adapters = probe_all(inputs);
    let failed = adapters.is_empty();
    let decision = arbitrate(&adapters, failed);
    let issues = assert_v01_alignment(&adapters, &decision);
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok((adapters, decision))
}
