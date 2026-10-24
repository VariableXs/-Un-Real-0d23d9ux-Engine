//! VE-F3421 · 域自检（判据逐条对应 `ver06a_skinchain.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四件（预演/影响面/编排/事务）→ `A21-四件-*`
//! - 两律（不中断 / 可回滚）→ `A21-两律-*`
//! - 预演预算不拖慢切换 → `A21-预算-预演*`
//! - 影响三分级 → `A21-影响-*`
//! - 阶段序（颜色先于几何）→ `A21-编排-*`
//! - 事务原子（无半新半旧）→ `A21-事务-*`
//! - 交互缓冲与回放 → `A21-交互-*`
//! - 读屏播报 → `A21-读屏-*`
//!
//! 纪律：判据区零 `unwrap()`/`expect()`；期望值为写死常量或判据侧独立重算。

use alloc::format;
use alloc::vec;
use super::ver06a_skinchain::*;
use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 标准三变更（颜色 + 尺寸 + 时长）。
fn std_deltas() -> Vec<TokenDelta> {
    vec![
        TokenDelta::new(
            "core:color.bg",
            SkinValue::Color(0x000000FF),
            SkinValue::Color(0x101418FF),
        ),
        TokenDelta::new("core:size.gap", SkinValue::Size(4), SkinValue::Size(8)),
        TokenDelta::new(
            "core:motion.fast",
            SkinValue::Duration(120),
            SkinValue::Duration(90),
        ),
    ]
}

/// 标准订阅表：color.* → 组件 2，size.* → 组件 1，motion.* → 组件 3。
fn std_registry() -> ImpactRegistry {
    let mut r = ImpactRegistry::new();
    r.subscribe("core:color.*", ComponentId(2));
    r.subscribe("core:size.*", ComponentId(1));
    r.subscribe("core:motion.*", ComponentId(3));
    r
}

/// 走到 Planning 阶段的引擎。
fn at_planning() -> SkinEngine {
    let mut e = SkinEngine::new();
    let _ = e.submit(std_deltas());
    let _ = e.rehearse(3);
    let _ = e.compute_impact(&std_registry());
    let _ = e.plan(2);
    e
}

/// 走到 Committing 阶段（快照已拍）。
fn at_committing() -> SkinEngine {
    let mut e = at_planning();
    let _ = e.begin_commit();
    e
}

/// VE-F3421 域自检。
pub fn run_ver06a_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ver06a");

    // ---- 四件 ----

    // 判据：预演逐条走同一校验路径，非法值在预演被拦。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(vec![TokenDelta::new(
            "core:weight.body",
            SkinValue::Weight(400),
            SkinValue::Weight(450),
        )]);
        let code = e.rehearse(2).err().map(|x| x.code);
        set.add(
            "A21-四件-预演拦非法值",
            code == Some(SkinCode::InvalidValue) && e.phase() == Phase::Aborted,
            "",
        );
    }

    // 判据：影响面按订阅表算出，而非全量。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(std_deltas());
        let _ = e.rehearse(3);
        let _ = e.compute_impact(&std_registry());
        // 三个键各命中一个组件，共 3 个。
        set.add(
            "A21-四件-影响面按订阅算",
            e.impact().len() == 3,
            "",
        );
    }

    // 判据：编排阶段数与类型数一致（三类型 → 三阶段）。
    {
        let e = at_planning();
        set.add("A21-四件-编排成阶", e.plan_ref().len() == 3, "");
    }

    // 判据：事务四步（begin → commit*n → animate → finish）可走通。
    {
        let mut e = at_committing();
        let n = e.plan_ref().len();
        let mut ok = true;
        for i in 0..n {
            if e.commit_stage(i, 1).is_err() {
                ok = false;
            }
        }
        ok = ok && e.begin_animate().is_ok() && e.finish().is_ok();
        set.add("A21-四件-事务四步走通", ok && e.phase() == Phase::Done, "");
    }

    // ---- 两律 ----

    // 判据：不中断——提交期间点击入队，完成后按到达序回放。
    {
        let mut e = at_committing();
        let _ = e.interaction(InteractionKind::Click, "btn.ok");
        let _ = e.interaction(InteractionKind::Key, "input.name");
        let _ = e.interaction(InteractionKind::Scroll, "list");
        let replay = e.replay_interactions();
        let ordered = replay.len() == 3
            && replay.get(0).map(|i| i.seq) == Some(0)
            && replay.get(1).map(|i| i.seq) == Some(1)
            && replay.get(2).map(|i| i.seq) == Some(2);
        set.add("A21-两律-不中断且按序回放", ordered, "");
    }

    // 判据：空闲期交互不入队（无需缓冲，直接生效）。
    {
        let mut e = SkinEngine::new();
        let buffered = e.interaction(InteractionKind::Click, "btn.ok");
        set.add(
            "A21-两律-空闲期不缓冲",
            buffered == Ok(false) && e.queue().len() == 0,
            "",
        );
    }

    // 判据：可回滚——提交一阶段后回滚，条目数 = 变更数。
    {
        let mut e = at_committing();
        let _ = e.commit_stage(0, 1);
        let n = e.rollback();
        set.add(
            "A21-两律-回滚条目等于变更数",
            n == Ok(3) && e.phase() == Phase::RolledBack,
            "",
        );
    }

    // 判据：回滚后不再持有快照（不留可二次回滚的幽灵态）。
    {
        let mut e = at_committing();
        let _ = e.rollback();
        let again = e.rollback().err().map(|x| x.code);
        set.add(
            "A21-两律-不可二次回滚",
            again == Some(SkinCode::BadPhase),
            "",
        );
    }

    // ---- 预算 ----

    // 判据：预演超预算即中止，且中止态不再接受提交。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(std_deltas());
        let _ = e.rehearse(REHEARSE_BUDGET_MS + 5);
        let next = e.begin_commit().err().map(|x| x.code);
        set.add(
            "A21-预算-预演超支即中止",
            next == Some(SkinCode::BadPhase) && e.phase() == Phase::Aborted,
            "",
        );
    }

    // 判据：预演耗时随变更数线性增长（判据侧独立重算 per = n + 2）。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(std_deltas());
        let _ = e.rehearse(0);
        let expect = std_deltas().len() as u32 + 2;
        set.add(
            "A21-预算-预演耗时线性",
            e.rehearse_cost() == expect,
            "",
        );
    }

    // 判据：提交累计超预算触发自动回滚（不是留在半提交态）。
    {
        let mut e = at_committing();
        let _ = e.commit_stage(0, APPLY_BUDGET_MS);
        let code = e.commit_stage(1, 1).err().map(|x| x.code);
        set.add(
            "A21-预算-提交超支自动回滚",
            code == Some(SkinCode::ApplyOverBudget) && e.phase() == Phase::RolledBack,
            "",
        );
    }

    // 判据：编排超预算中止。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(std_deltas());
        let _ = e.rehearse(3);
        let _ = e.compute_impact(&std_registry());
        let code = e.plan(PLAN_BUDGET_MS + 1).err().map(|x| x.code);
        set.add(
            "A21-预算-编排超支中止",
            code == Some(SkinCode::PlanOverBudget),
            "",
        );
    }

    // ---- 状态机 ----

    // 判据：Idle 直接 rehearse 被拒（不许跳预演）。
    {
        let mut e = SkinEngine::new();
        let code = e.rehearse(1).err().map(|x| x.code);
        set.add("A21-状态-不许跳预演", code == Some(SkinCode::BadPhase), "");
    }

    // 判据：未拍快照不许提交阶段。
    {
        let mut e = at_planning();
        let code = e.commit_stage(0, 1).err().map(|x| x.code);
        set.add(
            "A21-状态-未拍快照拒提交",
            code == Some(SkinCode::BadPhase),
            "",
        );
    }

    // 判据：跳阶段提交被拒（必须按序）。
    {
        let mut e = at_committing();
        let code = e.commit_stage(1, 1).err().map(|x| x.code);
        set.add(
            "A21-状态-跳阶段即拒",
            code == Some(SkinCode::StageOutOfRange),
            "",
        );
    }

    // 判据：阶段未提交完不许进动画。
    {
        let mut e = at_committing();
        let code = e.begin_animate().err().map(|x| x.code);
        set.add(
            "A21-状态-未提交完拒动画",
            code == Some(SkinCode::BadPhase),
            "",
        );
    }

    // 判据：终态不可再中止。
    {
        let mut e = at_committing();
        let _ = e.rollback();
        let code = e.abort().err().map(|x| x.code);
        set.add("A21-状态-终态拒中止", code == Some(SkinCode::BadPhase), "");
    }

    // ---- 校验与拒绝 ----

    // 判据：空变更集拒绝。
    {
        let mut e = SkinEngine::new();
        let code = e.submit(Vec::new()).err().map(|x| x.code);
        set.add("A21-校验-空集拒绝", code == Some(SkinCode::NoDelta), "");
    }

    // 判据：键缺命名空间即拒。
    {
        let mut e = SkinEngine::new();
        let code = e
            .submit(vec![TokenDelta::new(
                "color.bg",
                SkinValue::Color(1),
                SkinValue::Color(2),
            )])
            .err()
            .map(|x| x.code);
        set.add("A21-校验-键无命名空间即拒", code == Some(SkinCode::BadKey), "");
    }

    // 判据：新旧同值拒绝（无变化不进事务）。
    {
        let mut e = SkinEngine::new();
        let code = e
            .submit(vec![TokenDelta::new(
                "core:a",
                SkinValue::Size(4),
                SkinValue::Size(4),
            )])
            .err()
            .map(|x| x.code);
        set.add("A21-校验-无变化即拒", code == Some(SkinCode::NoChange), "");
    }

    // 判据：类型跨类变更拒绝（尺寸→颜色）。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(vec![TokenDelta::new(
            "core:a",
            SkinValue::Size(4),
            SkinValue::Color(0xFF),
        )]);
        let code = e.rehearse(1).err().map(|x| x.code);
        set.add(
            "A21-校验-跨类型变更即拒",
            code == Some(SkinCode::InvalidValue),
            "",
        );
    }

    // 判据：字重非百位步长拒绝。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(vec![TokenDelta::new(
            "core:w",
            SkinValue::Weight(400),
            SkinValue::Weight(650),
        )]);
        let code = e.rehearse(1).err().map(|x| x.code);
        set.add(
            "A21-校验-字重步长即拒",
            code == Some(SkinCode::InvalidValue),
            "",
        );
    }

    // ---- 影响分级 ----

    // 判据：三档分级映射固定（尺寸/阴影=全，颜色/字重=局部，时长/缓动=无感）。
    {
        let ok = grade_of(super::ver01e_typetree::TokenKind::Size)
            == ImpactGrade::FullRepaint
            && grade_of(super::ver01e_typetree::TokenKind::Shadow)
                == ImpactGrade::FullRepaint
            && grade_of(super::ver01e_typetree::TokenKind::Color)
                == ImpactGrade::PartialRepaint
            && grade_of(super::ver01e_typetree::TokenKind::Duration)
                == ImpactGrade::Noop;
        set.add("A21-影响-三档映射固定", ok, "");
    }

    // 判据：同一组件被多键命中时取更重的分级（不取最后一条）。
    {
        let mut reg = ImpactRegistry::new();
        reg.subscribe("core:color.bg", ComponentId(7));
        reg.subscribe("core:size.gap", ComponentId(7));
        let mut e = SkinEngine::new();
        let _ = e.submit(vec![
            TokenDelta::new(
                "core:color.bg",
                SkinValue::Color(1),
                SkinValue::Color(2),
            ),
            TokenDelta::new("core:size.gap", SkinValue::Size(1), SkinValue::Size(2)),
        ]);
        let _ = e.rehearse(3);
        let _ = e.compute_impact(&reg);
        let g = e.impact().components.first().map(|c| c.grade);
        set.add(
            "A21-影响-多键取最重",
            e.impact().len() == 1 && g == Some(ImpactGrade::FullRepaint),
            "",
        );
    }

    // 判据：无订阅的键不产生影响（影响面不会因空订阅膨胀）。
    {
        let mut reg = ImpactRegistry::new();
        reg.subscribe("other:key", ComponentId(9));
        let mut e = SkinEngine::new();
        let _ = e.submit(std_deltas());
        let _ = e.rehearse(3);
        let _ = e.compute_impact(&reg);
        set.add("A21-影响-无订阅零膨胀", e.impact().is_empty(), "");
    }

    // ---- 编排 ----

    // 判据：阶段序固定为 颜色→尺寸→字重→时长→缓动→阴影（六类全序）。
    {
        let use_k = super::ver01e_typetree::TokenKind::ALL;
        let mut sorted: Vec<u8> = Vec::new();
        for k in use_k.iter() {
            sorted.push(stage_order(*k));
        }
        let mut expect: Vec<u8> = Vec::new();
        for i in 0..6u8 {
            expect.push(i);
        }
        set.add("A21-编排-六类全序", sorted == expect, "");
    }

    // 判据：计划内键数与变更数相等（不重不漏）。
    {
        let e = at_planning();
        set.add("A21-编排-键数不重不漏", e.plan_ref().key_count() == 3, "");
    }

    // ---- 读屏与不变式 ----

    // 判据：读屏摘要含阶段名与四个计数。
    {
        let e = at_planning();
        let s = e.spoken_state();
        let ok = s.contains("编排中")
            && s.contains("3 条变更")
            && s.contains("3 个阶段")
            && s.contains("预演");
        set.add("A21-读屏-摘要含四计数", ok, "");
    }

    // 判据：完成态不变式自洽（提交数 = 计划数）。
    {
        let mut e = at_committing();
        let n = e.plan_ref().len();
        for i in 0..n {
            let _ = e.commit_stage(i, 1);
        }
        let _ = e.begin_animate();
        let _ = e.finish();
        set.add(
            "A21-不变式-完成态自洽",
            e.invariant_error().is_none() && e.phase() == Phase::Done,
            "",
        );
    }

    // 判据：拒绝全部入日志（零静默）。
    {
        let mut e = SkinEngine::new();
        let _ = e.submit(Vec::new());
        let _ = e.rehearse(1);
        let ok = e.rejects() == 2 && e.log().len() >= 2;
        set.add("A21-零静默-拒绝入日志", ok, "");
    }

    // 判据：诊断码两两互异。
    {
        let codes = [
            SkinCode::NoDelta,
            SkinCode::DeltaQuota,
            SkinCode::DuplicateDelta,
            SkinCode::BadKey,
            SkinCode::NoChange,
            SkinCode::InvalidValue,
            SkinCode::BadPhase,
            SkinCode::RehearseOverBudget,
            SkinCode::PlanOverBudget,
            SkinCode::ApplyOverBudget,
            SkinCode::ImpactTooLarge,
            SkinCode::InteractionOverflow,
            SkinCode::NoSnapshot,
            SkinCode::RollbackFailed,
            SkinCode::StageOutOfRange,
        ];
        let mut nums: Vec<u16> = Vec::new();
        let mut uniq = true;
        for c in codes.iter() {
            if nums.iter().any(|n| *n == c.code()) {
                uniq = false;
            }
            nums.push(c.code());
        }
        set.add(
            "A21-零静默-诊断码互异",
            uniq && codes.iter().all(|c| !c.advice().is_empty()),
            "",
        );
    }

    // 判据：交互队列溢出显性报错且计数可查。
    {
        let mut q = InteractionQueue::new();
        let mut last_err = None;
        for i in 0..(MAX_INTERACTIONS + 1) {
            let r = q.push(InteractionKind::Click, &alloc::format!("t{}", i));
            if let Err(e) = r {
                last_err = Some(e.code);
            }
        }
        set.add(
            "A21-交互-溢出显性报错",
            last_err == Some(SkinCode::InteractionOverflow) && q.dropped() == 1,
            "",
        );
    }

    set
}
