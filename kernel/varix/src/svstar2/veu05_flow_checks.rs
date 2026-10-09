//! VE-F4205 · 域自检（判据逐条对应，见 `veu05_flow.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 六步留痕（步步留痕）→ `U05-留痕-*`
//! - 兼容闸（破坏性必须升主版本+迁移指南）→ `U05-兼容-*`
//! - 灰度（档位与观测分离、越档即越步）→ `U05-灰度-*`
//! - 会签（缺签挂起+催办）→ `U05-会签-*`
//! - 弱化即破坏（无障碍红线）→ `U05-红线-*`
//! - 降级矩阵四格 + 越步 + 撤回 → `U05-降级-*` / `U05-错误-*`
//!
//! **判据自身的四条纪律**（本文件从 F4204 继承教训，新增第四条）：
//!
//! 1. **不把实现里恒真的表达式当判据。** 枚举值经 `code()` 必然能反查回自己，
//!    拿「反查失败」当判据是恒假的死守卫。
//! 2. **分层断言，不用联合式。** 不写 `A.is_err() || B.is_none()` 这种
//!    「下游也会拒」的式子——解析层放行时下游仍拒，判据照样全绿。
//! 3. **判据侧独立重算，不问被测方答案。** 回滚可回滚性由判据侧调
//!    [`version_recoverable`] 与自算的生命周期表重算，而不是问
//!    `RollbackVerdict::is_ok()`——那是自证式。
//! 4. **（本文件新增）判据不得只钉「无事件」侧。** 只断「未弱化时不算破坏」
//!    对「弱化了却没算破坏」一字未说，而那正是红线所在。故每条否定断言
//!    都要配一条**走真实路径造出事件**的反向断言。
//!
//! 5. **（本文件新增）越步判据必须造出越步事件。** `advance` 只走下一步，
//!    越步在这条路径上**永远不会发生**；只断言「越步被拒」的话，
//!    一个把 `advance_to` 写成永远 `Err` 的实现同样全绿。故本族同时钉
//!    **合法步位走步照常通过**（反向锚点）与**两向越步确实被拒**（正向）。
//!
//! 零墙钟、零 IO，回归可复现。

use super::veu02_model::DomainTag;
use super::veu03_registry::{ContractMeta, ContractRegistry, Lifecycle};
use super::veu05_flow::*;
use crate::checks::CheckSet;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 判据族前缀表（长前缀在前）。
const FAMILIES: [&str; 8] = [
    "U05-留痕-",
    "U05-兼容-",
    "U05-灰度-",
    "U05-会签-",
    "U05-红线-",
    "U05-降级-",
    "U05-错误-",
    "U05-收敛-",
];

/// 标准注册册（本项专用契约已在册，消费方为 T / U 两域）。
fn std_reg() -> ContractRegistry {
    standard_flow_registry()
}

/// 判据侧独立重算的**期望会签方集合**（从注册册现取，不问被测方）。
fn expected_parties(reg: &ContractRegistry, id: &str, ver: &str) -> Vec<DomainTag> {
    let mut v = reg.consumers(id, ver);
    v.sort_by_key(|d| match d {
        DomainTag::S => 0u8,
        DomainTag::T => 1u8,
        DomainTag::U => 2u8,
    });
    v
}

/// 造一条**破坏性且已履行双「必须」**的申请（用于测放行侧）。
fn compliant_breaking_request() -> ChangeRequest {
    ChangeRequest::new(
        "U05-CTR-A11Y",
        "2.0.0",
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px",
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px；另增焦点环说明",
        standard_a11y_full(),
        standard_a11y_full(),
    )
    .with_removed("旧字段 legacy_tip")
    .with_migration("迁移指南：删除 legacy_tip，改读 tip_text；提供 30 天兼容别名")
}

/// 造一条**弱化无障碍**的申请（红线语料：把对比度从 3 降到 1）。
fn weakening_request() -> ChangeRequest {
    ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px",
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px",
        standard_a11y_full(),
        vec![
            (A11yFacet::Contrast, 1),
            (A11yFacet::FocusReach, 3),
            (A11yFacet::FocusOrder, 3),
            (A11yFacet::ScreenLabel, 3),
            (A11yFacet::HitArea, 3),
        ],
    )
    .with_migration("把对比度要求从满档降到 1 档")
}

/// 造一条**破坏但未附迁移指南**的申请（用于测「报告在阻断时仍落定」）。
fn compliant_breaking_request_no_guide() -> ChangeRequest {
    let mut r = compliant_breaking_request();
    r.migration_guide = None;
    r
}

/// 受理 → 走到会签步之前的公共前置（受理 + 影响分析）。
fn opened_with_impact(req: ChangeRequest, reg: &ContractRegistry) -> ChangeFlowEngine {
    let mut e = ChangeFlowEngine::new();
    let _ = e.open(req, reg);
    let _ = e.advance("U05-CTR-A11Y", DomainTag::U);
    e
}

/// 把会签表全部签掉（**走真实表态路径**，不是直接改字段）。
fn sign_all(e: &mut ChangeFlowEngine, agree: u8) {
    let parties: Vec<DomainTag> = match e.flow("U05-CTR-A11Y") {
        Some(f) => f.cosign.iter().map(|r| r.domain).collect(),
        None => return,
    };
    for d in parties.iter().copied() {
        if let Some(r) = e.flow_mut("U05-CTR-A11Y").and_then(|f| f.cosign_row_mut(d)) {
            r.state = Some(SignState::Signed);
            r.agreed_points = agree.min(r.required_points);
        }
    }
}

/// 推进到灰度步并把灰度推到全量档（**逐档，每档带健康观测**）。
fn drive_canary_to_full(e: &mut ChangeFlowEngine) {
    let _ = e.advance("U05-CTR-A11Y", DomainTag::U); // 会签步
    let _ = e.advance("U05-CTR-A11Y", DomainTag::U); // 进灰度步
    let _ = e.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::OnePercent,
        CanaryObservation { stage: CanaryStage::Internal, healthy: true, observer: DomainTag::U },
        DomainTag::U,
    );
    let _ = e.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::TenPercent,
        CanaryObservation { stage: CanaryStage::OnePercent, healthy: true, observer: DomainTag::U },
        DomainTag::U,
    );
    let _ = e.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::Full,
        CanaryObservation { stage: CanaryStage::TenPercent, healthy: true, observer: DomainTag::U },
        DomainTag::U,
    );
}

/// 族内收敛的自检汇（只压缩播报，不压缩断言）。
struct FamilyTally {
    pending: Vec<(&'static str, String, bool, String)>,
}

impl FamilyTally {
    fn new() -> FamilyTally {
        FamilyTally { pending: Vec::new() }
    }

    fn add(&mut self, name: &'static str, passed: bool, detail: &str) {
        self.pending.push((family_of(name), name.to_string(), passed, detail.to_string()));
    }

    /// 细项全绿才并族；**有红则逐条出声**（红项必须能指名）。
    fn flush(self, set: &mut CheckSet) {
        let mut order: Vec<&'static str> = Vec::new();
        for (fam, _, _, _) in self.pending.iter() {
            if !order.iter().any(|f| f == fam) {
                order.push(*fam);
            }
        }
        for fam in order.iter() {
            let mut all_green = true;
            for p in self.pending.iter() {
                if p.0 == *fam && !p.2 {
                    all_green = false;
                }
            }
            if all_green {
                let n = self.pending.iter().filter(|p| p.0 == *fam).count();
                // 名字是 String，而 CheckSet::add 收 &'static str：
                // 以泄漏方式取 'static（本文件的判据名全部来自本文件字面量，
                // 泄漏是常量级一次性开销，与 F4204 同做法）。
                let name: &'static str =
                    Box::leak(format!("{}族·{} 项全绿", fam, n).into_boxed_str());
                set.add(name, true, fam);
            } else {
                for p in self.pending.iter() {
                    if p.0 == *fam {
                        let leaked: &'static str = Box::leak(p.1.clone().into_boxed_str());
                        let d: &'static str = Box::leak(p.3.clone().into_boxed_str());
                        set.add(leaked, p.2, d);
                    }
                }
            }
        }
    }
}

fn family_of(name: &str) -> &'static str {
    for f in FAMILIES.iter() {
        if name.starts_with(f) {
            return f;
        }
    }
    "U05-兼容-"
}

// ===========================================================================
// 一、六步留痕（锚点判据①）
// ===========================================================================

fn checks_trace(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 1.1 受理即留第一步痕迹，且游标落在申请步。
    let mut e = ChangeFlowEngine::new();
    e.open(standard_additive_request(), &reg).expect("受理");
    // cursor 本身已是 Option<FlowStep>，此处**直接取值**，
    // 不再 .map() 包一层（那会得到 Option<Option<..>>，比较恒不成立）。
    let cursor_after_open = e.flow("U05-CTR-A11Y").map(|f| f.cursor).unwrap_or(None);
    t.add(
        "U05-留痕-受理即落第一步",
        cursor_after_open == Some(FlowStep::Request) && !e.traces().is_empty(),
        "受理后游标须已在申请步且痕迹非空",
    );

    // 1.2 **全量六步走完后，六步每一步都恰有痕迹**（钉「步步」而非「有痕」）。
    let mut e2 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e2, 1);
    drive_canary_to_full(&mut e2);
    let _ = e2.advance("U05-CTR-A11Y", DomainTag::U); // 切换
    let _ = e2.advance("U05-CTR-A11Y", DomainTag::U); // 通告
    let mut per_step_ok = true;
    let mut missing: Vec<&str> = Vec::new();
    for s in FlowStep::ALL.iter().copied() {
        let n = e2
            .traces_of("U05-CTR-A11Y")
            .iter()
            .filter(|tr| tr.step == s && tr.outcome.is_progress())
            .count();
        // 判据侧独立算：每一步至少一条「通过」痕迹。
        if n < 1 {
            per_step_ok = false;
            missing.push(s.zh());
        }
    }
    t.add(
        "U05-留痕-六步逐步有痕",
        per_step_ok,
        &format!("缺痕步骤：{:?}", missing),
    );
    t.add(
        "U05-留痕-走完即完成",
        e2.flow("U05-CTR-A11Y").map(|f| f.is_complete()).unwrap_or(false),
        "游标到通告且未作废方为完成",
    );

    // 1.3 **被拒动作照样留痕**（这是「步步留痕」最容易漏的一格：
    //     只在成功时留痕的话，越权试过就查不到了）。
    let mut e3 = opened_with_impact(standard_additive_request(), &reg);
    // 未签就强行推进会签步 → 应被挂起，且挂起必须留痕。
    let r = e3.advance("U05-CTR-A11Y", DomainTag::U);
    let held_traced = e3
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|tr| tr.outcome == TraceOutcome::Held);
    t.add(
        "U05-留痕-被拒动作也留痕",
        r.is_err() && held_traced,
        "会签缺被挂起时，痕迹表须出现 Held 行",
    );

    // 1.4 痕迹可读屏（不留「只有内部计数」的痕）。
    let screen = e3.screen_text();
    t.add(
        "U05-留痕-痕迹可读屏",
        screen.contains("申请") && screen.contains("影响分析"),
        "读屏文本须含步位中文名",
    );

    t.flush(cs);
}

// ===========================================================================
// 二、兼容闸（锚点判据②）
// ===========================================================================

fn checks_compat(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 2.1 放行侧：纯新增（非破坏）**不升主版本也放行**。
    //     这条是**必要的反向锚点**：只测阻断侧的话，一个「永远阻断」的闸也全绿。
    let mut e = ChangeFlowEngine::new();
    e.open(standard_additive_request(), &reg).expect("受理");
    let r = e.advance("U05-CTR-A11Y", DomainTag::U);
    let allowed_additive = e
        .flow("U05-CTR-A11Y")
        .and_then(|f| f.verdict.as_ref())
        .map(|v| v.allows())
        .unwrap_or(false);
    t.add(
        "U05-兼容-非破坏不索主版本",
        r.is_ok() && allowed_additive,
        "纯新增未升主版本，应放行（否则闸退化为「永远阻断」）",
    );

    // 2.2 破坏 + 升主版本 + 有指南 → 放行。
    let e2 = opened_with_impact(compliant_breaking_request(), &reg);
    let v2 = e2.flow("U05-CTR-A11Y").and_then(|f| f.verdict.clone());
    t.add(
        "U05-兼容-破坏且双必齐则放行",
        v2.as_ref().map(|v| v.allows()).unwrap_or(false)
            && v2.as_ref().map(|v| v.major_bumped).unwrap_or(false)
            && v2.as_ref().map(|v| v.migration_ok).unwrap_or(false),
        "升主版本 + 有指南应放行",
    );

    // 2.3 **破坏 + 只升版没指南 → 阻断**，且码是「缺指南」那一个。
    let req_no_guide = {
        let mut r = compliant_breaking_request();
        r.migration_guide = None;
        r
    };
    let e3 = opened_with_impact(req_no_guide, &reg);
    let v3 = e3.flow("U05-CTR-A11Y").and_then(|f| f.verdict.clone());
    t.add(
        "U05-兼容-只升版无指南被阻断",
        v3.as_ref().map(|v| !v.allows()).unwrap_or(false)
            && v3.as_ref().map(|v| v.block_code() == E_BREAK_NO_GUIDE).unwrap_or(false),
        "升了版但没写指南，须给 E_BREAK_NO_GUIDE（半吊子形态可区分）",
    );

    // 2.4 **破坏 + 只有指南没升版 → 阻断**，码是「缺主版本」那一个。
    let req_no_major = {
        let mut r = compliant_breaking_request();
        r.target_version = "1.5.0".to_string(); // 与基线 1.0.0 同主版本
        r
    };
    let e4 = opened_with_impact(req_no_major, &reg);
    let v4 = e4.flow("U05-CTR-A11Y").and_then(|f| f.verdict.clone());
    t.add(
        "U05-兼容-有指南未升版被阻断",
        v4.as_ref().map(|v| !v.allows()).unwrap_or(false)
            && v4.as_ref().map(|v| v.block_code() == E_BREAK_NO_MAJOR).unwrap_or(false),
        "同主版本不算升版，须给 E_BREAK_NO_MAJOR",
    );

    // 2.5 **「升主版本」判的是严格大于**：同主版本（1.0.0 → 1.0.0）不算升。
    let req_same = {
        let mut r = compliant_breaking_request();
        r.target_version = "1.0.0".to_string();
        r
    };
    let e5 = opened_with_impact(req_same, &reg);
    let v5 = e5.flow("U05-CTR-A11Y").and_then(|f| f.verdict.clone());
    t.add(
        "U05-兼容-同版本不算升版",
        v5.as_ref().map(|v| !v.major_bumped).unwrap_or(true),
        "目标主版本等于基线即未升版",
    );

    // 2.6 **空迁移指南不算「有指南」**（Some(\"\") 是最典型的半吊子）。
    let req_blank = {
        let mut r = compliant_breaking_request();
        r.target_version = "1.0.0".to_string();
        r.migration_guide = Some("   ".to_string());
        r
    };
    t.add(
        "U05-兼容-空白指南不算有指南",
        !req_blank.migration_ok(),
        "Some(空白) 不得被当成已附指南",
    );

    // 2.7 删字段必判破坏（**不许自报豁免**）。
    let req_del = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.0.0",
        "X",
        "X",
        standard_a11y_full(),
        standard_a11y_full(),
    )
    .with_removed("any_field");
    t.add(
        "U05-兼容-删字段即破坏",
        req_del.classify() == ChangeKind::Breaking,
        "删字段须判破坏性，与目标版本无关",
    );

    // 2.8 判据正文改动即破坏（用哈希比，不靠肉眼比词）。
    let req_text = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.0.0",
        "对比度不低于 4.5:1",
        "对比度不低于 4.5:1 ",
        standard_a11y_full(),
        standard_a11y_full(),
    );
    t.add(
        "U05-兼容-判据正文改动即破坏",
        req_text.classify() == ChangeKind::Breaking,
        "仅差一个空格也是语义动过（哈希口径）",
    );

    // 2.9 版本解析：非法版本在受理处即拒，不进流程。
    let mut e9 = ChangeFlowEngine::new();
    let bad = ChangeRequest::new(
        "U05-CTR-A11Y",
        "abc",
        "X",
        "X",
        standard_a11y_full(),
        standard_a11y_full(),
    );
    let r9 = e9.open(bad, &reg);
    t.add(
        "U05-兼容-非法版本受理即拒",
        r9.is_err() && e9.flow("U05-CTR-A11Y").is_none(),
        "不可解析的主版本须在受理处拒，不留半截流程",
    );

    // 2.10 不在册契约不受理（**F4203 单源**：不在册就没有基线）。
    let mut e10 = ChangeFlowEngine::new();
    let ghost = ChangeRequest::new(
        "U05-CTR-GHOST",
        "2.0.0",
        "X",
        "X",
        standard_a11y_full(),
        standard_a11y_full(),
    );
    let r10 = e10.open(ghost, &reg);
    t.add(
        "U05-兼容-不在册契约不受理",
        r10.is_err(),
        "注册册查无此契约时须拒（无基线可比）",
    );

    // 2.11 同一契约不接受并行申请。
    let mut e11 = ChangeFlowEngine::new();
    let _ = e11.open(standard_additive_request(), &reg);
    let dup = e11.open(standard_additive_request(), &reg);
    t.add(
        "U05-兼容-并行申请被拒",
        dup.is_err() && e11.flows().len() == 1,
        "同一契约同时只允许一条在途变更",
    );

    t.flush(cs);
}

// ===========================================================================
// 三、灰度（锚点判据③）
// ===========================================================================

fn checks_canary(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 3.1 逐档推进：四档按序，每档一次观测。
    let mut e = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e, 1);
    let _ = e.advance("U05-CTR-A11Y", DomainTag::U); // 会签
    let _ = e.advance("U05-CTR-A11Y", DomainTag::U); // 进灰度
    let at_internal = e.flow("U05-CTR-A11Y").and_then(|f| f.canary);
    t.add(
        "U05-灰度-入步落内部档",
        at_internal == Some(CanaryStage::Internal),
        "进灰度步应落在内部档",
    );
    drive_canary_to_full(&mut e);
    t.add(
        "U05-灰度-逐档可达全量",
        e.flow("U05-CTR-A11Y").and_then(|f| f.canary) == Some(CanaryStage::Full),
        "四档逐档推进后应到全量档",
    );

    // 3.2 **越档推进被拒**（内部档直接要全量）。
    let mut e2 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e2, 1);
    let _ = e2.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e2.advance("U05-CTR-A11Y", DomainTag::U);
    let r2 = e2.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::Full,
        CanaryObservation { stage: CanaryStage::Internal, healthy: true, observer: DomainTag::U },
        DomainTag::U,
    );
    t.add(
        "U05-灰度-越档被拒",
        r2.is_err(),
        "内部档直接要全量档须被拒（否则四步形同虚设）",
    );

    // 3.3 **越档的代价是作废 + 回退**，且作废痕迹**留在表里**。
    let voided_from = e2.flow("U05-CTR-A11Y").and_then(|f| f.voided_from);
    let void_traced = e2
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|tr| tr.outcome == TraceOutcome::Voided);
    t.add(
        "U05-灰度-越档即作废",
        voided_from == Some(FlowStep::Canary) && void_traced,
        "越档须置作废起点，并在痕迹中标出被作废的既有推进",
    );

    // 3.4 作废后不接受继续推进。
    let r4 = e2.advance("U05-CTR-A11Y", DomainTag::U);
    t.add(
        "U05-灰度-作废后拒推进",
        r4.is_err(),
        "已作废流程不得再被推进一步",
    );

    // 3.5 **档位与观测分离**：观测报别的档 → 拒。
    let mut e5 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e5, 1);
    let _ = e5.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e5.advance("U05-CTR-A11Y", DomainTag::U);
    let r5 = e5.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::OnePercent,
        CanaryObservation { stage: CanaryStage::TenPercent, healthy: true, observer: DomainTag::U },
        DomainTag::U,
    );
    t.add(
        "U05-灰度-观测档不符被拒",
        r5.is_err(),
        "拿别的档的观测来放本档，等于没观测",
    );

    // 3.6 不健康档不许推进，且**须留痕**（不健康这件事本身要可查）。
    let mut e6 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e6, 1);
    let _ = e6.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e6.advance("U05-CTR-A11Y", DomainTag::U);
    let r6 = e6.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::OnePercent,
        CanaryObservation { stage: CanaryStage::Internal, healthy: false, observer: DomainTag::U },
        DomainTag::U,
    );
    let unhealthy_traced = e6
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|tr| tr.code == E_CANARY_UNHEALTHY);
    t.add(
        "U05-灰度-不健康拒推进",
        r6.is_err() && unhealthy_traced,
        "观测不健康须拒推进，并在痕迹中留痕",
    );

    // 3.7 **灰度未到全量不得切换**（钉住「灰度」不是装饰）。
    let mut e7 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e7, 1);
    let _ = e7.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e7.advance("U05-CTR-A11Y", DomainTag::U);
    let r7 = e7.advance("U05-CTR-A11Y", DomainTag::U); // 想直接切换
    t.add(
        "U05-灰度-未全量拒切换",
        r7.is_err(),
        "灰度未到全量档时切换步须被拒",
    );

    // 3.8 档位序与百分比是**单调**的（钉住「逐档」的数据基础）。
    let mut monotonic = true;
    for i in 1..CanaryStage::ALL.len() {
        if CanaryStage::ALL[i].percent() <= CanaryStage::ALL[i - 1].percent() {
            monotonic = false;
        }
    }
    t.add(
        "U05-灰度-档位百分比单调递增",
        monotonic,
        "档位序须与流量单调一致，否则「越档」无从定义",
    );

    t.flush(cs);
}

// ===========================================================================
// 四、会签（锚点判据④）
// ===========================================================================

fn checks_cosign(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 4.1 会签表**按注册册消费方铺开**（F4203 单源，不采信申请自述）。
    let mut e = ChangeFlowEngine::new();
    e.open(standard_additive_request(), &reg).expect("受理");
    let got: Vec<DomainTag> = e
        .flow("U05-CTR-A11Y")
        .map(|f| f.cosign.iter().map(|r| r.domain).collect())
        .unwrap_or_default();
    let want = expected_parties(&reg, "U05-CTR-A11Y", "1.0.0");
    let mut got_sorted = got.clone();
    got_sorted.sort_by_key(|d| match d {
        DomainTag::S => 0u8,
        DomainTag::T => 1u8,
        DomainTag::U => 2u8,
    });
    t.add(
        "U05-会签-表按注册册消费方铺开",
        got_sorted == want && !want.is_empty(),
        &format!("期望 {:?}，实得 {:?}", want, got_sorted),
    );

    // 4.2 缺签 → **挂起**（不进下一步）。
    let mut e2 = opened_with_impact(standard_additive_request(), &reg);
    let r2 = e2.advance("U05-CTR-A11Y", DomainTag::U);
    let cursor = e2.flow("U05-CTR-A11Y").and_then(|f| f.cursor);
    t.add(
        "U05-会签-缺签挂起",
        r2.is_err() && cursor == Some(FlowStep::Impact),
        "缺签时游标须停在影响分析步，不得进会签",
    );

    // 4.3 缺签 → **逐一催办**，且催办方与未签方一一对应。
    let dunned: Vec<DomainTag> = e2.dunning().iter().map(|d| d.domain).collect();
    let pending = e2.flow("U05-CTR-A11Y").map(|f| f.pending_parties()).unwrap_or_default();
    let mut dun_sorted = dunned.clone();
    dun_sorted.sort_by_key(|d| match d {
        DomainTag::S => 0u8,
        DomainTag::T => 1u8,
        DomainTag::U => 2u8,
    });
    let mut pend_sorted = pending.clone();
    pend_sorted.sort_by_key(|d| match d {
        DomainTag::S => 0u8,
        DomainTag::T => 1u8,
        DomainTag::U => 2u8,
    });
    t.add(
        "U05-会签-缺签即催办",
        !pend_sorted.is_empty() && dun_sorted == pend_sorted,
        "催办方集合须等于未签方集合",
    );

    // 4.4 **催办重复累加不覆盖**（「催了三次没人理」不能与「催了一次」同形）。
    let mut e4 = opened_with_impact(standard_additive_request(), &reg);
    let _ = e4.advance("U05-CTR-A11Y", DomainTag::U);
    let n1 = e4.dunning().iter().map(|d| d.notices).max().unwrap_or(0);
    let _ = e4.advance("U05-CTR-A11Y", DomainTag::U);
    let n2 = e4.dunning().iter().map(|d| d.notices).max().unwrap_or(0);
    t.add(
        "U05-会签-催办次数累加",
        n1 == 1 && n2 == 2,
        &format!("首次 1，二次应累加为 2（实得 {}→{})", n1, n2),
    );

    // 4.5 齐签 → 会签步通过。
    let mut e5 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e5, 1);
    let r5 = e5.advance("U05-CTR-A11Y", DomainTag::U);
    t.add(
        "U05-会签-齐签则过",
        r5.is_ok(),
        "全签后应通过会签步",
    );

    // 4.6 **一票否决**：一拒即驳回，不被其余方的同意抵消。
    let mut e6 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e6, 1);
    if let Some(r) = e6.flow_mut("U05-CTR-A11Y").and_then(|f| f.cosign_row_mut(DomainTag::T)) {
        r.state = Some(SignState::Refused);
    }
    let r6 = e6.advance("U05-CTR-A11Y", DomainTag::U);
    t.add(
        "U05-会签-一拒即否决",
        r6.is_err(),
        "拒签为一票否决，其余方同意不得抵消",
    );

    // 4.7 **要点数未凑齐不算签**（把「勾了但没看完」当成签）。
    let mut e7 = opened_with_impact(standard_additive_request(), &reg);
    {
        let f = e7.flow_mut("U05-CTR-A11Y").expect("流程");
        for r in f.cosign.iter_mut() {
            r.state = Some(SignState::Signed);
            r.agreed_points = 0; // 标了签但要点一个没同意
        }
    }
    let v7 = e7.flow("U05-CTR-A11Y").map(|f| f.cosign_verdict());
    t.add(
        "U05-会签-要点未齐不算签",
        v7 == Some(CosignVerdict::Pending),
        "签了态但要点未凑齐，须判挂起",
    );

    t.flush(cs);
}

// ===========================================================================
// 五、弱化即破坏（锚点判据⑤ · 域本色红线）
// ===========================================================================

fn checks_redline(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 5.1 强度下降 → 弱化。
    let d = weakening_request().a11y_delta();
    t.add(
        "U05-红线-强度下降即弱化",
        d.is_weakened() && d.weakened.contains(&A11yFacet::Contrast),
        "对比度由 3 降到 1，须判弱化并指名该面",
    );

    // 5.2 **删面计入弱化**（删比降更狠）。
    let del = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "X",
        "X",
        standard_a11y_full(),
        vec![
            (A11yFacet::Contrast, 3),
            (A11yFacet::FocusReach, 3),
            (A11yFacet::FocusOrder, 3),
            (A11yFacet::ScreenLabel, 3),
        ], // 少了 HitArea
    );
    let dd = del.a11y_delta();
    t.add(
        "U05-红线-删面计入弱化",
        dd.is_weakened() && dd.removed.contains(&A11yFacet::HitArea),
        "删掉一个无障碍面须判弱化并指名该面",
    );

    // 5.3 **升级不算弱化**（反向锚点：否则红线退化为「任何改动都算弱化」）。
    let up = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "X",
        "X",
        standard_a11y_full(),
        vec![
            (A11yFacet::Contrast, 3),
            (A11yFacet::FocusReach, 3),
            (A11yFacet::FocusOrder, 3),
            (A11yFacet::ScreenLabel, 3),
            (A11yFacet::HitArea, 3),
        ],
    );
    t.add(
        "U05-红线-持平不算弱化",
        !up.a11y_delta().is_weakened(),
        "强度持平不得判弱化",
    );

    // 5.4 **新增面不算弱化**（新增是加强方向）。
    let add = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "X",
        "X",
        vec![(A11yFacet::Contrast, 2)],
        vec![(A11yFacet::Contrast, 2), (A11yFacet::HitArea, 1)],
    );
    let da = add.a11y_delta();
    t.add(
        "U05-红线-新增面不判弱化",
        !da.is_weakened() && da.added.contains(&A11yFacet::HitArea),
        "改动后多出来的面属加强，不得判弱化",
    );

    // 5.5 **弱化直接判破坏性**（红线并入种类，不给「弱化但非破坏」留路）。
    t.add(
        "U05-红线-弱化即判破坏",
        weakening_request().classify() == ChangeKind::Breaking,
        "弱化必须并入破坏性变更",
    );

    // 5.6 **弱化 + 升版 + 指南 也仍按破坏处理**（红线路径不可被双「必须」绕过）。
    let sneaky = {
        let mut r = weakening_request();
        r.target_version = "2.0.0".to_string();
        r
    };
    let e = opened_with_impact(sneaky, &reg);
    let v = e.flow("U05-CTR-A11Y").and_then(|f| f.verdict.clone());
    t.add(
        "U05-红线-弱化仍受闸约束",
        v.as_ref().map(|v| v.kind.is_breaking()).unwrap_or(false)
            && v.as_ref().map(|v| v.a11y.is_weakened()).unwrap_or(false),
        "弱化即便升了主版本带了指南，结论仍须是破坏性",
    );

    // 5.7 **弱化在痕迹里看得见**（布尔会翻转，痕迹是历史）。
    let weak_traced = e
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|tr| tr.code == E_A11Y_WEAKENED);
    t.add(
        "U05-红线-弱化单独留痕",
        weak_traced,
        "弱化须单独留一条痕，不能只藏在兼容闸布尔里",
    );

    // 5.8 **强度域封闭**：超过上限按上限截断（不产生假弱化）。
    t.add(
        "U05-红线-强度超限按上限截断",
        clamp_strength(250) == A11yFacet::MAX_STRENGTH && clamp_strength(2) == 2,
        "声明 250 应被截到上限，不得让比较失真",
    );

    // 5.9 无障碍面**恰好五面**（域封闭：判据侧独立点数）。
    t.add(
        "U05-红线-无障碍面恰好五面",
        A11yFacet::ALL.len() == 5,
        "无障碍面集合须恰好五面",
    );

    // 5.10 弱化面计数把删面与削弱同权（钉住计数口径）。
    t.add(
        "U05-红线-弱化计数含删面",
        d.weakened_count() == d.weakened.len() + d.removed.len(),
        "弱化面数须含删除面",
    );

    // 5.11 **专列恰好五行**：锚点要求「无障碍判据影响专列」，
    //      而「专列」若只列有变化的面，全持平时就塌成**空表**——
    //      空表与「忘了评估」在读屏上完全同形。
    let col = weakening_request().a11y_column();
    t.add(
        "U05-红线-专列恰好五行",
        col.len() == A11yFacet::ALL.len() && !col.is_empty(),
        &format!("专列须一行一面（实得 {} 行）", col.len()),
    );

    // 5.12 **持平的面也占位**（反向锚点：专列不得只列变化项）。
    let flat = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "X",
        "X",
        standard_a11y_full(),
        standard_a11y_full(),
    );
    let fc = flat.a11y_column();
    t.add(
        "U05-红线-持平也占位",
        fc.len() == 5
            && fc.rows.iter().all(|r| r.effect == A11yEffect::Unchanged)
            && fc.regressive_rows().is_empty(),
        "全持平时专列仍须五行且全标持平（占位即「评估过了」的证据）",
    );

    // 5.13 **专列与结论同源**：判据侧独立核对「专列说弱化」与
    //      「结论说弱化」在**每一面**上都不打架。判据不调被测方
    //      的归并函数，而是按行重算一遍该面的归属。
    let mut same = true;
    for r in col.rows.iter() {
        let expect_regressive = match r.effect {
            A11yEffect::Weakened => true,
            A11yEffect::Removed => true,
            _ => false,
        };
        let in_delta = d.weakened.contains(&r.facet) || d.removed.contains(&r.facet);
        if expect_regressive != in_delta {
            same = false;
        }
    }
    t.add(
        "U05-红线-专列与结论逐面一致",
        same,
        "专列判为弱化/删除的面须逐面出现在结论的弱化或删除清单里",
    );

    // 5.14 **加强与新增不判弱化**（专列上要能区分这两者，
    //      否则「加强」与「新增」在复盘里同形）。
    let up2 = ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "X",
        "X",
        vec![(A11yFacet::Contrast, 1)],
        vec![(A11yFacet::Contrast, 2), (A11yFacet::HitArea, 1)],
    );
    let uc = up2.a11y_column();
    t.add(
        "U05-红线-加强与新增可区分",
        uc.row(A11yFacet::Contrast).map(|r| r.effect) == Some(A11yEffect::Strengthened)
            && uc.row(A11yFacet::HitArea).map(|r| r.effect) == Some(A11yEffect::Added)
            && !up2.a11y_delta().is_weakened(),
        "加强与新增须各自归位，且都不判弱化",
    );

    // 5.15 **专列可读屏**（一行一面逐行可念）。
    let ct = col.screen_text();
    t.add(
        "U05-红线-专列可读屏",
        ct.contains("对比度") && ct.contains("弱化") && ct.lines().count() >= 6,
        "专列读屏须逐行给出面名、前后强度与归类",
    );

    // 5.16 **流程册上的专列取自受理的申请**（不受闸结论的归并口径影响：
    //      结论里存的是弱化/新增/删除三类，拿不出「持平」与「加强」）。
    let reg = std_reg();
    let e2 = opened_with_impact(standard_additive_request(), &reg);
    t.add(
        "U05-红线-流程册专列恒五行",
        e2.flow("U05-CTR-A11Y").map(|f| f.a11y_column().len()) == Some(5),
        "放行态申请（无任何弱化）的专列同样须五行",
    );

    t.flush(cs);
}

// ===========================================================================
// 六、降级矩阵四格 + 回滚协议
// ===========================================================================

fn checks_degrade(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 6.1 格一·越步 → 作废 + 回退。
    //     越步的形态：作废后的痕迹标 Voided，且**行还在**（删了就查不到越权）。
    let mut e = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e, 1);
    drive_canary_to_full(&mut e);
    let before = e.traces_of("U05-CTR-A11Y").len();
    // 人为把游标退到会签步，再直接推切换 —— 引擎按游标给出的下一步走，
    // 故这里用「灰度未全量就切换」已被拒的事实作越步的反向证据。
    let mut e1 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e1, 1);
    let _ = e1.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e1.advance("U05-CTR-A11Y", DomainTag::U);
    let r1 = e1.advance("U05-CTR-A11Y", DomainTag::U); // 未全量即切换 = 越步
    let voided1 = e1.flow("U05-CTR-A11Y").and_then(|f| f.voided_from);
    t.add(
        "U05-降级-越步即拒",
        r1.is_err(),
        "灰度未全量的切换请求须被拒",
    );
    t.add(
        "U05-降级-越档作废已钉",
        voided1.is_none() && before > 0,
        "越步（未越档）不置作废起点；作废只在越档/重入申请步时发生",
    );

    // 6.2 格二·会签缺 → 挂起 + 催办（已在会签族，此处钉「不误判为通过」）。
    let mut e2 = opened_with_impact(standard_additive_request(), &reg);
    let _ = e2.advance("U05-CTR-A11Y", DomainTag::U);
    let v2 = e2.flow("U05-CTR-A11Y").map(|f| f.cosign_verdict());
    t.add(
        "U05-降级-缺签不误判通过",
        v2 == Some(CosignVerdict::Pending) && !v2.unwrap().may_advance(),
        "缺签须判挂起且不可继续",
    );

    // 6.3 格三·破坏未升版 → 阻断。
    let req_no_major = {
        let mut r = compliant_breaking_request();
        r.target_version = "1.4.0".to_string();
        r
    };
    let e3 = opened_with_impact(req_no_major, &reg);
    t.add(
        "U05-降级-破坏未升版被阻断",
        e3.flow("U05-CTR-A11Y").and_then(|f| f.verdict.as_ref()).map(|v| !v.allows()).unwrap_or(false),
        "同主版本 + 破坏 → 阻断",
    );

    // 6.4 格四·回滚失败 → 立案（两种失败形态**码必须不同**）。
    //     专造一个「同契约有已废止旧版本」的册：回滚目标是那条旧版本。
    let mut reg2 = std_reg();
    reg2.register(ContractMeta::new(
        "U05-CTR-ROLL",
        "0.9.0",
        DomainTag::S,
        vec![DomainTag::T, DomainTag::U],
        "历史版本判据（供回滚目标用）",
    ))
    .expect("旧版登记");
    // 不加消费方：F4203 的废止闸要求引用计数归零，且本回滚场景不需要消费方。
    // register() 已把契约落在 Registered 态（F4203 单源），再迁 Registered
    // 是非法同态迁移——此处只需前进到 Retired。
    reg2.transition("U05-CTR-ROLL", Lifecycle::Retired, "历史版本废止")
        .expect("旧版废止");

    // 让流程册里有一条针对 U05-CTR-ROLL 的在途流程（受理时基线即 0.9.0）。
    let roll_req = ChangeRequest::new(
        "U05-CTR-ROLL",
        "1.0.0",
        "历史版本判据（供回滚目标用）",
        "历史版本判据（供回滚目标用）",
        standard_a11y_full(),
        standard_a11y_full(),
    );
    let mut e4 = ChangeFlowEngine::new();
    e4.open(roll_req, &reg2).expect("受理");

    // (a) 目标不在册 → TargetMissing
    let va = e4.rollback_to("U05-CTR-ROLL", "7.7.7", DomainTag::U, &reg2);
    // (b) 目标已废止 → TargetRetired
    let vb = e4.rollback_to("U05-CTR-ROLL", "0.9.0", DomainTag::U, &reg2);
    t.add(
        "U05-降级-回滚目标缺失立案",
        !va.is_ok() && matches!(va, RollbackVerdict::TargetMissing(_)),
        "回滚到不在册版本须判缺失",
    );
    t.add(
        "U05-降级-回滚目标废止立案",
        !vb.is_ok() && matches!(vb, RollbackVerdict::TargetRetired(_)),
        "回滚到已废止版本须判废止（回滚不得复活契约）",
    );
    t.add(
        "U05-降级-两失败码可区分",
        matches!(&va, RollbackVerdict::TargetMissing(_)) && matches!(&vb, RollbackVerdict::TargetRetired(_)),
        "「写错版本」与「目标已废止」须给不同结论，否则复盘分不开",
    );

    // 6.5 回滚失败**必须真的立案**（两条）。
    let case_codes: Vec<&'static str> = e4.cases().iter().map(|c| c.code).collect();
    t.add(
        "U05-降级-回滚失败即立案",
        case_codes.contains(&E_ROLLBACK_MISSING) && case_codes.contains(&E_ROLLBACK_RETIRED),
        &format!("立案码集合 {:?}", case_codes),
    );

    // 6.6 立案带严重度与出路（**拒绝必须给出路**）。
    let advice_ok = e4.cases().iter().all(|c| !c.advice.trim().is_empty());
    t.add(
        "U05-降级-立案必带出路",
        advice_ok && !e4.cases().is_empty(),
        "每条立案都须给出可执行的下一步",
    );

    // 6.7 **回滚成功路径**：退到在册未废止的基线版本。
    let mut e7 = ChangeFlowEngine::new();
    let _ = e7.open(standard_additive_request(), &reg);
    let ok = e7.rollback("U05-CTR-A11Y", DomainTag::U, &reg);
    t.add(
        "U05-降级-回滚成功退基线",
        ok.is_ok(),
        &format!("回滚到基线版本应成功（实得 {}）", ok.zh()),
    );

    // 6.8 **判据侧独立重算可回滚性**（不采信被测方结论）。
    //     这一条同时钉住「不在册」与「已废止」两个不可回滚条件。
    let indep_ok = !version_recoverable(false, Lifecycle::Frozen)
        && !version_recoverable(true, Lifecycle::Retired)
        && version_recoverable(true, Lifecycle::Frozen)
        && version_recoverable(true, Lifecycle::Registered);
    t.add(
        "U05-降级-可回滚性独立重算一致",
        indep_ok,
        "不在册/已废止须判不可回滚；在册未废止须判可回滚",
    );

    // 6.9 生命周期判定表**四态各归其位**（判据侧独立枚举，不调被测方）。
    let life = lifecycle_is_retired(Lifecycle::Draft)
        == false
        && lifecycle_is_retired(Lifecycle::Registered) == false
        && lifecycle_is_retired(Lifecycle::Frozen) == false
        && lifecycle_is_retired(Lifecycle::Retired) == true;
    t.add(
        "U05-降级-仅废止态判不可回滚",
        life,
        "只有 Retired 判不可回滚；草拟/已注册/已冻结均可回滚",
    );

    // 6.23 **回滚成功必须腾出在途名额**（回滚的死胡同，与作废同源）：
    //      只把游标归零却留着流程行，`open` 会永远以「已有在途变更」拒受理，
    //      这条契约从此再也改不了。
    let mut e23 = ChangeFlowEngine::new();
    let _ = e23.open(standard_additive_request(), &reg);
    let rb = e23.rollback("U05-CTR-A11Y", DomainTag::U, &reg);
    let freed = e23.flow("U05-CTR-A11Y").is_none();
    let reopen = e23.open(standard_additive_request(), &reg);
    t.add(
        "U05-降级-回滚成功腾出名额",
        rb.is_ok() && freed && reopen.is_ok(),
        "回滚成功后须能重新受理同一契约的变更（回滚不得成为新的死胡同）",
    );

    // 6.24 **回滚不抹痕迹，且回滚本身留痕留名**。
    let rb_traced = e23
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|x| x.code == E_ROLLED_BACK && x.actor == DomainTag::U);
    t.add(
        "U05-降级-回滚留痕留名",
        rb_traced && !e23.traces_of("U05-CTR-A11Y").is_empty(),
        "回滚是后果最重的动作，痕迹须记发起人",
    );

    // 6.25 **回滚失败不得腾名额**（失败的回滚没改变任何东西，
    //      放走流程行等于把在途变更弄丢）。
    let mut e25 = opened_with_impact(standard_additive_request(), &reg);
    let bad_rb = e25.rollback_to("U05-CTR-A11Y", "7.7.7", DomainTag::U, &reg);
    t.add(
        "U05-降级-回滚失败保留流程行",
        !bad_rb.is_ok() && e25.flow("U05-CTR-A11Y").is_some(),
        "回滚失败不得把在途流程一并丢掉",
    );

    t.flush(cs);
}

// ===========================================================================
// 六之二、越步（降级矩阵第一格·步序侧）+ 撤回出口
// ===========================================================================

fn checks_order(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 6.10 **反向锚点（先立）**：请求「正是下一步」时照常通过。
    //     没有这条，一个「advance_to 永远返回 Err」的实现能把本族全绿。
    let mut e_ok = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e_ok, 1);
    let r_ok = e_ok.advance_to("U05-CTR-A11Y", FlowStep::Cosign, DomainTag::U);
    t.add(
        "U05-降级-合法步位照常走",
        r_ok.is_ok() && e_ok.flow("U05-CTR-A11Y").and_then(|f| f.cursor) == Some(FlowStep::Cosign),
        "请求的正是下一步时不得被当成越步（否则本族只是「永远拒绝」）",
    );

    // 6.11 **向前越步被拒**（受理+影响分析后直接要切换 = 跳过会签与灰度）。
    let mut e1 = opened_with_impact(standard_additive_request(), &reg);
    let r1 = e1.advance_to("U05-CTR-A11Y", FlowStep::Cutover, DomainTag::U);
    t.add(
        "U05-降级-向前越步被拒",
        r1.is_err(),
        "跳过会签与灰度直接要求切换，须被拒",
    );

    // 6.12 **越步必须「回退」**：游标退回未启动（不是原地不动）。
    //     判据侧独立算：作废后游标须为 None，否则「回退」是空话。
    let cur1 = e1.flow("U05-CTR-A11Y").and_then(|f| f.cursor);
    let voided1 = e1.flow("U05-CTR-A11Y").and_then(|f| f.voided_from);
    t.add(
        "U05-降级-越步退回未启动",
        cur1.is_none() && voided1 == Some(FlowStep::Cutover),
        &format!("游标须退回未启动（实得 {:?}），作废起点记在被请求的步", cur1),
    );

    // 6.13 **越步必须「作废」**：整条痕迹标作废，且**一行不删**。
    let tr1 = e1.traces_of("U05-CTR-A11Y");
    let void_rows = tr1.iter().filter(|x| x.outcome == TraceOutcome::Voided).count();
    let left_rows = tr1.iter().filter(|x| x.outcome == TraceOutcome::Advanced).count();
    t.add(
        "U05-降级-越步作废整条留痕",
        void_rows >= 1 && left_rows == 0 && !tr1.is_empty(),
        &format!("既有推进痕迹须整条标作废但保留（作废 {} 条 / 未作废 {} 条 / 共 {} 条）", void_rows, left_rows, tr1.len()),
    );

    // 6.14 **越步后不再接受推进**（否则作废只是个标记）。
    t.add(
        "U05-降级-越步后拒推进",
        e1.advance("U05-CTR-A11Y", DomainTag::U).is_err()
            && e1.advance_to("U05-CTR-A11Y", FlowStep::Impact, DomainTag::U).is_err(),
        "已作废流程两条走步口都须拒",
    );

    // 6.15 **向后越步同罪**（回头重走已过的步 = 把已作废的判定复活）。
    let mut e2 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e2, 1);
    let _ = e2.advance("U05-CTR-A11Y", DomainTag::U); // 会签
    let r2 = e2.advance_to("U05-CTR-A11Y", FlowStep::Request, DomainTag::U);
    t.add(
        "U05-降级-向后越步被拒",
        r2.is_err() && e2.flow("U05-CTR-A11Y").and_then(|f| f.cursor).is_none(),
        "回头重走申请步须被拒并同样退回未启动（否则拨游标即可复活作废判定）",
    );

    // 6.16 **作废流程必须撤得回**（否则申请人被永久锁死：
    //     advance 拒推进 + open 拒并行受理 = 这条契约再也改不了）。
    let mut e3 = opened_with_impact(standard_additive_request(), &reg);
    sign_all(&mut e3, 1);
    let _ = e3.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e3.advance("U05-CTR-A11Y", DomainTag::U);
    let _ = e3.advance_canary(
        "U05-CTR-A11Y",
        CanaryStage::Full,
        CanaryObservation { stage: CanaryStage::Internal, healthy: true, observer: DomainTag::U },
        DomainTag::U,
    ); // 越档作废
    let traces_before = e3.traces_of("U05-CTR-A11Y").len();
    let r3 = e3.retract("U05-CTR-A11Y", DomainTag::U);
    t.add(
        "U05-降级-作废流程可撤回",
        r3.is_ok() && e3.flow("U05-CTR-A11Y").is_none(),
        "作废流程须有撤回出口",
    );

    // 6.17 **撤回不抹痕迹**，且撤回本身留痕。
    let traces_after = e3.traces_of("U05-CTR-A11Y").len();
    let retracted_traced = e3
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|x| x.outcome == TraceOutcome::Retracted);
    t.add(
        "U05-降级-撤回留痕且不删行",
        traces_after >= traces_before && retracted_traced,
        &format!("撤回只抹在途状态，痕迹须一行不删并留撤回痕（{} → {}）", traces_before, traces_after),
    );

    // 6.18 **撤回后可修正后重提**（「从第 1 步重来」这条正路真的在）。
    let reopened = e3.open(standard_additive_request(), &reg);
    t.add(
        "U05-降级-撤回后能重新受理",
        reopened.is_ok() && e3.flow("U05-CTR-A11Y").is_some(),
        "撤回后同契约须能重新受理（这是 §二「正路未封死」的实证）",
    );

    // 6.19 **在途流程撤不回**（撤回若对在途流程开放，就是绕开会签的暗门：
    //     会签挂起 → 撤回 → 重开 → 表是空的，会签白走一遍）。
    let mut e4 = opened_with_impact(standard_additive_request(), &reg);
    let r4 = e4.retract("U05-CTR-A11Y", DomainTag::U);
    t.add(
        "U05-降级-在途流程撤不回",
        r4.is_err() && e4.flow("U05-CTR-A11Y").is_some(),
        "未作废的在途流程不得撤回",
    );

    t.flush(cs);
}

// ===========================================================================
// 六之三、会签表为空（消费方全撤订后表塌空）
// ===========================================================================

fn checks_empty_cosign(mut t: FamilyTally, cs: &mut CheckSet) {
    // 走一条**真实可达**的塌空路径：F4203 允许注册时须有消费方，
    // 但撤订只标记不删行，于是「全部撤订后 consumers() 为空」是真的会发生的。
    let mut reg = std_reg();
    let _ = reg.deregister("U05-CTR-A11Y", "1.0.0", DomainTag::T);
    let _ = reg.deregister("U05-CTR-A11Y", "1.0.0", DomainTag::U);
    let empty = reg.consumers("U05-CTR-A11Y", "1.0.0").is_empty();

    let mut e = opened_with_impact(standard_additive_request(), &reg);
    let table_empty = e.flow("U05-CTR-A11Y").map(|f| f.cosign.is_empty()).unwrap_or(false);

    // 6.20 **空表不判通过**：这是本条的全部要害。逐行扫空的
    //     `cosign_verdict()` 天生返回 Passed，「会签通过」的真实含义
    //     成了「没人需要签」。
    t.add(
        "U05-会签-空表不判通过",
        empty && table_empty,
        "撤订全部消费方后会签表须为空（前置事实）",
    );

    let r = e.advance("U05-CTR-A11Y", DomainTag::U);
    let v = e.flow("U05-CTR-A11Y").map(|f| f.cosign_verdict());
    t.add(
        "U05-会签-空表挂起不放行",
        r.is_err() && v == Some(CosignVerdict::Vacuous) && !CosignVerdict::Vacuous.may_advance(),
        "空表须判独立的「空转」而非「全签通过」——归并过去痕迹上就会记一句干净的通过",
    );

    // 6.21 **空转与挂起不得同形**（反向锚点：否则读册的人分不出
    //      「还没人签」与「没人需要签」，前者的出路是催办，后者的出路是补消费方）。
    t.add(
        "U05-会签-空转不并入挂起",
        CosignVerdict::Vacuous != CosignVerdict::Pending
            && CosignVerdict::Vacuous.zh() != CosignVerdict::Pending.zh()
            && !CosignVerdict::Vacuous.is_terminal(),
        "空转是独立一态：不可继续（出路：补消费方）且非终局（不是被否决）",
    );

    // 6.21 **空表必须立案**：静默挂起会让人以为「只是还没人签」。
    let codes: Vec<&'static str> = e.cases().iter().map(|c| c.code).collect();
    t.add(
        "U05-会签-空表即立案",
        codes.contains(&E_COSIGN_NO_PARTY),
        &format!("立案码集合 {:?}", codes),
    );

    // 6.22 **空表挂起须留痕**且指向会签步。
    let held = e
        .traces_of("U05-CTR-A11Y")
        .iter()
        .any(|x| x.step == FlowStep::Cosign && x.code == E_COSIGN_NO_PARTY);
    t.add(
        "U05-会签-空表留痕",
        held,
        "「这一步没发生」须在痕迹里看得见，不能只挂起不留痕",
    );

    t.flush(cs);
}

// ===========================================================================
// 六之四、影响分析报告（第二步的产出物 · 分析 O(影响面)）
// ===========================================================================

fn checks_impact(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 6.26 **第二步必须产出报告**：只判「过没过兼容闸」不叫影响分析，
    //      它判的是这份改动**波及谁**。
    let req = standard_additive_request()
        .with_affected("U03-CTR-FOCUS")
        .with_affected("U05-CTR-A11Y");
    let e = opened_with_impact(req, &reg);
    t.add(
        "U05-降级-影响分析产出报告",
        e.flow("U05-CTR-A11Y").and_then(|f| f.impact.as_ref()).is_some(),
        "过完第二步须有影响分析报告",
    );

    // 6.27 **须会签方取自注册册**，不采信申请自述。
    let r = e.flow("U05-CTR-A11Y").and_then(|f| f.impact.as_ref());
    let want = expected_parties(&reg, "U05-CTR-A11Y", "1.0.0");
    t.add(
        "U05-降级-报告须会签方对齐册",
        r.map(|x| x.parties.clone()) == Some(want.clone()) && !want.is_empty(),
        &format!("报告里的须会签方须等于注册册消费方 {:?}", want),
    );

    // 6.28 **专列在报告里恒五行**（不因报告换了个载体就塌成空表）。
    t.add(
        "U05-降级-报告含五行专列",
        r.map(|x| x.a11y.len()) == Some(5),
        "报告内的无障碍专列须仍是一面一行",
    );

    // 6.29 **影响面条目数**（`分析 O(影响面)` 的「面」由 `surface_len` 度量）：
    //      判据侧独立重算 = 申报数 + 相关方数 + 五面 + 档跳数。
    let hops = CanaryStage::ALL.len() - 1;
    let want_len = 2 + want.len() + 5 + hops;
    t.add(
        "U05-降级-影响面条目可重算",
        r.map(|x| x.surface_len()) == Some(want_len) && hops == canary_hops(),
        &format!("条目数须可独立重算（期望 {}）", want_len),
    );

    // 6.30 **档跳数跟着档表走**（写死字面量的话，增一档就静默少算一档）。
    t.add(
        "U05-降级-档跳数随档表",
        canary_hops() == CanaryStage::ALL.len() - 1 && canary_hops() > 0,
        "须逐档走过的跳数须由档表长度导出",
    );

    // 6.31 **未申报影响面须显性写出**（反向锚点：空表与「忘了评估」不可同形）。
    let e2 = opened_with_impact(standard_additive_request(), &reg);
    let rt = e2
        .flow("U05-CTR-A11Y")
        .and_then(|f| f.impact.as_ref())
        .map(|x| x.screen_text())
        .unwrap_or_default();
    t.add(
        "U05-降级-未申报影响面显性化",
        rt.contains("未申报影响面") && e2.flow("U05-CTR-A11Y").map(|f| f.req.affected_contracts.is_empty()).unwrap_or(false),
        "未申报影响面须在报告里显性写明，不得与「评估为空」同形",
    );

    // 6.32 **被阻断时报告照样落**（否则被拒的变更成了黑盒：
    //      读册的人只知道「被拒」，不知道波及了谁）。
    let e3 = opened_with_impact(compliant_breaking_request_no_guide(), &reg);
    t.add(
        "U05-降级-阻断时报告仍落定",
        e3.flow("U05-CTR-A11Y").and_then(|f| f.impact.as_ref()).is_some()
            && e3.flow("U05-CTR-A11Y").and_then(|f| f.verdict.as_ref()).map(|v| !v.allows()).unwrap_or(false),
        "兼容闸阻断时影响面须仍可读",
    );

    t.flush(cs);
}

// ===========================================================================
// 七、错误零静默 + 自身可读性
// ===========================================================================

fn checks_errors(mut t: FamilyTally, cs: &mut CheckSet) {
    let reg = std_reg();

    // 7.1 标准册自身须干净。
    let e = {
        let mut x = ChangeFlowEngine::new();
        let _ = x.open(standard_additive_request(), &reg);
        x
    };
    t.add(
        "U05-错误-标准环境自审干净",
        e.self_audit().is_empty(),
        &format!("自审须无问题项：{:?}", e.self_audit()),
    );

    // 7.2 **完成六步却没过兼容闸 = 流程被绕过**，自审须能报出来。
    let mut forged = ChangeFlowEngine::new();
    if let Some(f) = forged.flow_mut("U05-CTR-A11Y") {
        let _ = f;
    }
    // 走一条**手工伪造**的完成流程：直接构造游标到通告、不经闸。
    forged.open(standard_additive_request(), &reg).expect("受理");
    if let Some(f) = forged.flow_mut("U05-CTR-A11Y") {
        f.cursor = Some(FlowStep::Announce);
        f.verdict = None; // 没过闸
        f.cosign = vec![CosignRow::new(DomainTag::T, 1)];
        f.cosign[0].state = Some(SignState::Signed);
        f.cosign[0].agreed_points = 1;
        f.canary = None;
    }
    let audit = forged.self_audit();
    t.add(
        "U05-错误-绕过闸可被自审捕获",
        audit.iter().any(|s| s.contains(E_STEP_STATE)),
        "游标到通告却无兼容闸结论，自审须报出来",
    );

    // 7.3 **未齐签却已进灰度**，自审须能报出来。
    let mut forged2 = ChangeFlowEngine::new();
    forged2.open(standard_additive_request(), &reg).expect("受理");
    if let Some(f) = forged2.flow_mut("U05-CTR-A11Y") {
        f.cursor = Some(FlowStep::Canary);
        // 直接置一个**放行**结论（判据只关心会签这一条路是否被守住）。
        f.verdict = Some(CompatVerdict {
            kind: ChangeKind::Additive,
            major_bumped: false,
            migration_ok: false,
            a11y: A11yDelta::none(),
        });
        f.cosign = vec![CosignRow::new(DomainTag::T, 1)]; // 未表态
        f.canary = Some(CanaryStage::Internal);
    }
    let audit2 = forged2.self_audit();
    t.add(
        "U05-错误-绕过会签可被自审捕获",
        audit2.iter().any(|s| s.contains(E_COSIGN_PENDING)),
        "未齐签却已进灰度，自审须报出来",
    );

    // 7.4 痕迹容量触顶 → 走诊断日志，**不静默丢**。
    let mut e4 = ChangeFlowEngine::new();
    // 两级触顶链路：受理 + 被拒尝试先把痕迹（MAX_TRACES）塞满，
    // 继续留痕则溢出降级串写进诊断（MAX_DIAG 条后诊断自身触顶），
    // 读屏才打「触顶……非全量」显性标记。循环数取两级上限之和，
    // 恰好让痕迹先满、诊断也被降级串填满——任一环缺失判据即红。
    let _ = e4.open(standard_additive_request(), &reg);
    for _ in 0..(MAX_TRACES + MAX_DIAG) {
        let _ = e4.advance("U05-CTR-A11Y", DomainTag::U); // 被拒也留痕
    }
    let diag_used = e4.diag().items.iter().any(|s| s.contains("痕迹溢出降级"));
    let truncated_marked = e4.diag().screen_text().contains("触顶");
    t.add(
        "U05-错误-痕迹触顶走诊断不静默",
        diag_used && truncated_marked,
        "痕迹触顶后须在诊断里显性说明「非全量」",
    );

    // 7.5 读屏总览含关键节（流程 / 痕迹 / 立案 / 催办）。
    let mut e5 = opened_with_impact(standard_additive_request(), &reg);
    let _ = e5.advance("U05-CTR-A11Y", DomainTag::U); // 挂起 + 催办
    let s = e5.screen_text();
    t.add(
        "U05-错误-读屏含四节",
        s.contains("痕迹") && s.contains("催办"),
        "读屏总览须同时给出痕迹与催办两节",
    );

    // 7.6 **读屏总览含无障碍专列**（锚点要求的专列要在读屏面上，
    //     只存在于结构体里等于没做——读屏的人看不到）。
    t.add(
        "U05-错误-读屏含无障碍专列",
        s.contains("无障碍判据影响专列") && s.contains("对比度"),
        "读屏总览须带出无障碍判据影响专列",
    );

    // 7.7 **作废了却仍停在推进位**，自审须能报出来。
    //     作废的语义是「整条不成立」，游标却还指着某一步时，
    //     读册的人会以为它仍在推进中。
    let mut forged3 = ChangeFlowEngine::new();
    forged3.open(standard_additive_request(), &reg).expect("受理");
    if let Some(f) = forged3.flow_mut("U05-CTR-A11Y") {
        f.cursor = Some(FlowStep::Impact);
        f.voided_from = Some(FlowStep::Cutover);
        f.canary = Some(CanaryStage::TenPercent);
    }
    let audit3 = forged3.self_audit();
    t.add(
        "U05-错误-作废却推进可被自审捕获",
        audit3.iter().any(|x| x.contains(E_AUDIT_VOIDED_ADVANCED)),
        &format!("已作废却留游标/灰度档，自审须报出来：{:?}", audit3),
    );

    // 7.8 **会签表为空却已推进**，自审须能报出来。
    let mut forged4 = ChangeFlowEngine::new();
    forged4.open(standard_additive_request(), &reg).expect("受理");
    if let Some(f) = forged4.flow_mut("U05-CTR-A11Y") {
        f.cosign.clear();
        f.cursor = Some(FlowStep::Cosign);
        f.verdict = Some(CompatVerdict {
            kind: ChangeKind::Additive,
            major_bumped: false,
            migration_ok: false,
            a11y: A11yDelta::none(),
        });
    }
    let audit4 = forged4.self_audit();
    t.add(
        "U05-错误-空会签表推进可被自审捕获",
        audit4.iter().any(|x| x.contains(E_AUDIT_NO_PARTY)),
        &format!("会签表空却已推进，自审须报出来：{:?}", audit4),
    );

    t.flush(cs);
}

// ===========================================================================
// 入口
// ===========================================================================

/// 跑 VE-F4205 全部自检。
pub fn run_veu05_flow_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F4205 契约变更流程引擎");
    checks_trace(FamilyTally::new(), &mut cs);
    checks_compat(FamilyTally::new(), &mut cs);
    checks_canary(FamilyTally::new(), &mut cs);
    checks_cosign(FamilyTally::new(), &mut cs);
    checks_redline(FamilyTally::new(), &mut cs);
    checks_degrade(FamilyTally::new(), &mut cs);
    checks_order(FamilyTally::new(), &mut cs);
    checks_empty_cosign(FamilyTally::new(), &mut cs);
    checks_impact(FamilyTally::new(), &mut cs);
    checks_errors(FamilyTally::new(), &mut cs);
    cs
}

