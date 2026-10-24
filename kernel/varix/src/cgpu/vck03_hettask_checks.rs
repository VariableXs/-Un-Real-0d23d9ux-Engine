//! vck03_hettask_checks — CGPU-F1603 异构任务模型与标签 · 域自检判据。
//!
//! 判据原文（施工书锚点 CGPU-F1603）：任务模型：异构任务扩展（字段 schema
//! 版本化——B 域结构扩展不改内核契约声明）；功耗标签复用（F1458 三维标签
//! 跨引擎适用——NPU 功耗系数表）；引擎亲和性（NPU-only/prefer-NPU/any——
//! any 时调度器按能效选引擎）；批处理任务（批维度参数）；实时约束（DSP
//! deadline 错过=失败不静默）；任务校验（引擎能力×任务需求匹配——校验规则
//! 表。判据：扩展不改核、标签统一、亲和三级、批模型、deadline、校验、六组。
//!
//! 六组对齐锚点测试词：字段 schema / 标签跨引擎 / 亲和 / 批模型 / 实时 /
//! 校验。判据纪律：零 panic 面（无 unwrap/expect，Result 用 matches!）；
//! 预期常量独立给定；outer_steps 向上取整与 ETA 链逐值手算复核
//! （eta = max(ops/gran,1) × 10 × max(steps,1)，除数均非零常量路径）。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::cgpu::vck02_hetres::{EngineKind, FEAT_BATCH, FEAT_REALTIME};
use crate::cgpu::vck03_hettask::{
    Affinity, BatchDim, DispatchOutcome, HetScheduler, HetTaskExt, PwrLabel, PwrLevel,
    TaskValidator, EFFICIENCY_ORDER, HET_TASK_SCHEMA, NPU_PWR_COEFF_MW, SUPPORTED_SCHEMA_MINOR,
    XcCode,
};

/// 构造最小合法任务（判据场景的基线样本——逐字段显式，不隐藏默认值）。
fn base_task() -> HetTaskExt {
    HetTaskExt {
        schema_minor: SUPPORTED_SCHEMA_MINOR,
        affinity: Affinity::Any,
        label: PwrLabel { level: PwrLevel::Balanced, delayable: false, mergeable: false },
        ops_required: 1000,
        model_id: Some(7),
        granularity_ops: 100,
        batch: None,
        deadline_us: None,
    }
}

/// 候选集基线：NPU 全能力 / DSP 仅实时 / ISP 裸。
fn base_candidates() -> Vec<(EngineKind, u32)> {
    alloc::vec![
        (EngineKind::Npu, FEAT_BATCH | FEAT_REALTIME),
        (EngineKind::Dsp, FEAT_REALTIME),
        (EngineKind::Isp, 0),
    ]
}

/// 判据主入口（注册于 cgpu/mod.rs blocks 数组，tag CGPU-F1603）。
pub fn run_vck03_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-f1603");

    // --- 组一：字段 schema（判据一）---
    // 扩-01：扩展不改核的结构形态——侧车八字段齐备 + schema 版本锚定 +
    // 版本协商闭集（声明 ≤1 通过、=2 拒 HtSchemaUnsupported）。
    let t = base_task();
    let schema_ok = HET_TASK_SCHEMA == "K03-hettask-v1" && SUPPORTED_SCHEMA_MINOR == 1;
    let mut over = base_task();
    over.schema_minor = 2;
    let over_rejected = matches!(over.static_valid(), Err(XcCode::HtSchemaUnsupported));
    let base_ok = matches!(t.static_valid(), Ok(()));
    if schema_ok && over_rejected && base_ok {
        s.ok("F1603-扩-01-schema版本协商闭集");
    } else {
        s.fail("F1603-扩-01-schema版本协商闭集", "schema 协商失效");
    }

    // --- 组一：字段 schema（判据二）---
    // 扩-02：引擎特定参数必带项——粒度为零拒 HtGranZero；有算子量缺模型 ID
    // 拒 HtModelMissing；带模型 ID 或零算子量放行（双路径闭合）。
    let mut zero_gran = base_task();
    zero_gran.granularity_ops = 0;
    let mut no_model = base_task();
    no_model.model_id = None;
    let mut idle_ok = base_task();
    idle_ok.ops_required = 0;
    idle_ok.model_id = None;
    let field_ok = matches!(zero_gran.static_valid(), Err(XcCode::HtGranZero))
        && matches!(no_model.static_valid(), Err(XcCode::HtModelMissing))
        && matches!(idle_ok.static_valid(), Ok(()));
    if field_ok {
        s.ok("F1603-扩-02-引擎参数必带校验");
    } else {
        s.fail("F1603-扩-02-引擎参数必带校验", "必带项校验失效");
    }

    // --- 组二：标签跨引擎（判据三）---
    // 签-01：四档闭集 ordinal 连续互异 + 中文名互异 + NPU 系数表逐档对拍
    // （判据"NPU 功耗系数表"独立常量 [180,450,900,1500]）。
    let all = PwrLevel::ALL;
    let ord_ok = all[0].ordinal() == 0
        && all[1].ordinal() == 1
        && all[2].ordinal() == 2
        && all[3].ordinal() == 3;
    let names = [all[0].zh(), all[1].zh(), all[2].zh(), all[3].zh()];
    let names_unique = names[0] != names[1]
        && names[0] != names[2]
        && names[0] != names[3]
        && names[1] != names[2]
        && names[1] != names[3]
        && names[2] != names[3];
    let coeff_ok = NPU_PWR_COEFF_MW[0] == 180
        && NPU_PWR_COEFF_MW[1] == 450
        && NPU_PWR_COEFF_MW[2] == 900
        && NPU_PWR_COEFF_MW[3] == 1500;
    let probe = PwrLabel { level: PwrLevel::Performance, delayable: false, mergeable: true };
    let query_ok = probe.npu_coeff_mw() == 900 && NPU_PWR_COEFF_MW.len() == 4;
    if ord_ok && names_unique && coeff_ok && query_ok {
        s.ok("F1603-签-01-NPU功耗系数表四档");
    } else {
        s.fail("F1603-签-01-NPU功耗系数表四档", "系数表失效");
    }

    // --- 组二：标签跨引擎（判据四）---
    // 签-02：三维标签语义——Boost 不可延迟互斥（Boost+delayable 拒
    // HtLabelInvalid；Boost 即跑放行；非 Boost 任意组合放行）+
    // F1458 三维字段齐备（level/delayable/mergeable）。
    let boost_delay = PwrLabel { level: PwrLevel::Boost, delayable: true, mergeable: false };
    let boost_now = PwrLabel { level: PwrLevel::Boost, delayable: false, mergeable: false };
    let saver_delay = PwrLabel { level: PwrLevel::Saver, delayable: true, mergeable: true };
    let label_ok = !boost_delay.valid()
        && boost_now.valid()
        && saver_delay.valid();
    let mut boost_task = base_task();
    boost_task.label = boost_delay;
    let reject_via_task = matches!(boost_task.static_valid(), Err(XcCode::HtLabelInvalid));
    if label_ok && reject_via_task {
        s.ok("F1603-签-02-三维标签Boost互斥");
    } else {
        s.fail("F1603-签-02-三维标签Boost互斥", "标签语义失效");
    }

    // --- 组三：亲和三级（判据五）---
    // 亲-01：三级闭集中文名互异 + 能效序闭集对拍（NPU>DSP>MEDIA>ISP——
    // 判据"any 时调度器按能效选引擎"的排序键）。
    let aff_names = [Affinity::NpuOnly.zh(), Affinity::PreferNpu.zh(), Affinity::Any.zh()];
    let aff_ok = aff_names[0] != aff_names[1]
        && aff_names[0] != aff_names[2]
        && aff_names[1] != aff_names[2];
    let eff_ok = EFFICIENCY_ORDER[0] == EngineKind::Npu
        && EFFICIENCY_ORDER[1] == EngineKind::Dsp
        && EFFICIENCY_ORDER[2] == EngineKind::Media
        && EFFICIENCY_ORDER[3] == EngineKind::Isp
        && EFFICIENCY_ORDER.len() == 4;
    if aff_ok && eff_ok {
        s.ok("F1603-亲-01-亲和三级与能效序");
    } else {
        s.fail("F1603-亲-01-亲和三级与能效序", "亲和闭集失效");
    }

    // --- 组三：亲和三级（判据六）---
    // 亲-02：NpuOnly 语义——候选无 NPU → validate 显性 Err(HtAffinityUnsat)；
    // 候选有 NPU → NPU 行可承接、非 NPU 行逐行 deny 同码；pick 只选 NPU。
    let npu_only = base_task_with_affinity(Affinity::NpuOnly);
    let r_no_npu = TaskValidator::validate(&alloc::vec![(EngineKind::Dsp, FEAT_REALTIME)], &npu_only, 0);
    let unsat = matches!(r_no_npu, Err(XcCode::HtAffinityUnsat));
    let rows = TaskValidator::validate(&base_candidates(), &npu_only, 0);
    let rows_ok = match &rows {
        Ok(rs) => {
            rs.len() == 3
                && rs[0].kind == EngineKind::Npu
                && rs[0].eligible
                && rs[0].deny.is_none()
                && rs[1].deny == Some(XcCode::HtAffinityUnsat)
                && rs[2].deny == Some(XcCode::HtAffinityUnsat)
                && !rs[1].eligible
                && !rs[2].eligible
        }
        Err(_) => false,
    };
    let picked_npu = match &rows {
        Ok(rs) => TaskValidator::pick(rs, Affinity::NpuOnly) == Some(EngineKind::Npu),
        Err(_) => false,
    };
    if unsat && rows_ok && picked_npu {
        s.ok("F1603-亲-02-NpuOnly显性不可满足");
    } else {
        s.fail("F1603-亲-02-NpuOnly显性不可满足", "NpuOnly 语义失效");
    }

    // --- 组四：批模型（判据七）---
    // 批-01：批维度合法性三拒一过（batch=0 / micro=0 / micro>batch 拒，
    // 合法通过）+ 外层步数向上取整手算四例：
    // (10,3)→(10+2)/3=4；(9,3)→3 整除；(1,1)→1；(7,7)→1。
    let b1 = BatchDim { batch: 10, micro_batch: 3 };
    let b2 = BatchDim { batch: 9, micro_batch: 3 };
    let b3 = BatchDim { batch: 1, micro_batch: 1 };
    let b4 = BatchDim { batch: 7, micro_batch: 7 };
    let bad1 = BatchDim { batch: 0, micro_batch: 1 };
    let bad2 = BatchDim { batch: 5, micro_batch: 0 };
    let bad3 = BatchDim { batch: 3, micro_batch: 5 };
    let steps_ok = b1.outer_steps() == 4
        && b2.outer_steps() == 3
        && b3.outer_steps() == 1
        && b4.outer_steps() == 1;
    let valid_ok = b1.valid() && !bad1.valid() && !bad2.valid() && !bad3.valid();
    if steps_ok && valid_ok {
        s.ok("F1603-批-01-批维度与外层步数");
    } else {
        s.fail("F1603-批-01-批维度与外层步数", "批模型失效");
    }

    // --- 组四：批模型（判据八）---
    // 批-02：批任务落无 FEAT_BATCH 引擎 → 逐行 deny HtBatchCapMiss 不可
    // 承接（Dsp 仅实时无批、Isp 裸）→ pick 全拒 → NoEngine 回执。
    let mut batch_task = base_task();
    batch_task.batch = Some(BatchDim { batch: 8, micro_batch: 4 });
    let rb = TaskValidator::validate(&base_candidates(), &batch_task, 0);
    let batch_deny = match &rb {
        Ok(rs) => {
            rs[0].eligible // NPU 有 FEAT_BATCH
                && rs[1].deny == Some(XcCode::HtBatchCapMiss)
                && rs[2].deny == Some(XcCode::HtBatchCapMiss)
                && TaskValidator::pick(rs, Affinity::Any) == Some(EngineKind::Npu)
        }
        Err(_) => false,
    };
    // 全候选无批能力：候选收紧到 DSP/ISP → pick None。
    let rb2 = TaskValidator::validate(
        &alloc::vec![(EngineKind::Dsp, FEAT_REALTIME), (EngineKind::Isp, 0)],
        &batch_task,
        0,
    );
    let all_denied = match &rb2 {
        Ok(rs) => rs.iter().all(|r| !r.eligible) && TaskValidator::pick(rs, Affinity::Any).is_none(),
        Err(_) => false,
    };
    if batch_deny && all_denied {
        s.ok("F1603-批-02-无批能力引擎拒绝");
    } else {
        s.fail("F1603-批-02-无批能力引擎拒绝", "批能力校验失效");
    }

    // --- 组五：实时（判据九）---
    // 实-01：deadline 提交前拦——已过（now ≥ dl）显性 Err(HtDeadlinePassed)
    // 不静默；实时任务落无 FEAT_REALTIME 引擎逐行 deny。
    let mut late = base_task();
    late.deadline_us = Some(1000);
    let r_late = TaskValidator::validate(&base_candidates(), &late, 1000);
    let r_edge = TaskValidator::validate(&base_candidates(), &late, 999);
    let late_ok = matches!(r_late, Err(XcCode::HtDeadlinePassed)) && r_edge.is_ok();
    let mut rt_task = base_task();
    rt_task.deadline_us = Some(10_000);
    let rr = TaskValidator::validate(&base_candidates(), &rt_task, 0);
    let rt_deny = match &rr {
        Ok(rs) => rs[0].eligible && rs[1].eligible && rs[2].deny == Some(XcCode::HtRealtimeCapMiss),
        Err(_) => false,
    };
    if late_ok && rt_deny {
        s.ok("F1603-实-01-deadline提交前拦");
    } else {
        s.fail("F1603-实-01-deadline提交前拦", "deadline 拦截失效");
    }

    // --- 组五：实时（判据十）---
    // 实-02：调度面显性回执——ETA 链手算：
    // ops=20000 gran=100 → ops_win=200 → eta=200×10×1=2000µs；
    // now=0 + eta=2000 > dl=1000 → DeadlineMissed{at_us:2000}，missed+1。
    // 边界恰值：eta=1000 恰等于 dl=1000 不算错过（> 语义）→ Dispatched。
    let mut sched = HetScheduler::new(base_candidates());
    let mut heavy = base_task();
    heavy.ops_required = 20_000;
    heavy.deadline_us = Some(1000);
    let out_miss = sched.submit(&heavy, 0);
    let miss_ok = match out_miss {
        Ok(DispatchOutcome::DeadlineMissed { at_us }) => at_us == 2000,
        _ => false,
    };
    let mut edge = base_task();
    edge.ops_required = 10_000; // ops_win=100 → eta=1000 恰等于 dl
    edge.deadline_us = Some(1000);
    let out_edge = sched.submit(&edge, 0);
    let edge_ok = matches!(out_edge, Ok(DispatchOutcome::Dispatched { engine: EngineKind::Npu, eta_us: 1000 }));
    if miss_ok && edge_ok && sched.missed == 1 {
        s.ok("F1603-实-02-错过即失败显性回执");
    } else {
        s.fail("F1603-实-02-错过即失败显性回执", "显性回执失效");
    }

    // --- 组六：校验（判据十一）---
    // 校-01：规则表次序——静态自检先于 deadline（schema 超版+已过期双违
    // → 报 schema 码不报 deadline 码）；候选空集 → Err 显性拒绝。
    let mut double_bad = base_task();
    double_bad.schema_minor = 9;
    double_bad.deadline_us = Some(1);
    let r_order = TaskValidator::validate(&base_candidates(), &double_bad, 9999);
    let order_ok = matches!(r_order, Err(XcCode::HtSchemaUnsupported));
    let r_empty = TaskValidator::validate(&alloc::vec![], &base_task(), 0);
    let empty_ok = matches!(r_empty, Err(XcCode::HtAffinityUnsat));
    if order_ok && empty_ok {
        s.ok("F1603-校-01-规则次序与空集拒绝");
    } else {
        s.fail("F1603-校-01-规则次序与空集拒绝", "规则表失效");
    }

    // --- 组六：校验（判据十二）---
    // 校-02：prefer-NPU 选路——NPU 可承接必选 NPU；NPU 缺席候选按能效序
    // 降级到 DSP（降级路留痕：eff_rank 最小者）；MatchRow eff_rank 对拍
    // （NPU=0/DSP=1/MEDIA=2/ISP=3，闭集外 u32::MAX）。
    let rows_pref = TaskValidator::validate(&base_candidates(), &base_task(), 0);
    let pref_ok = match &rows_pref {
        Ok(rs) => {
            TaskValidator::pick(rs, Affinity::PreferNpu) == Some(EngineKind::Npu)
                && rs[0].eff_rank == 0
                && rs[1].eff_rank == 1
                && rs[2].eff_rank == 3
        }
        Err(_) => false,
    };
    let rows_no_npu = TaskValidator::validate(
        &alloc::vec![(EngineKind::Dsp, FEAT_REALTIME), (EngineKind::Media, 0)],
        &base_task(),
        0,
    );
    let degrade_ok = match &rows_no_npu {
        Ok(rs) => TaskValidator::pick(rs, Affinity::PreferNpu) == Some(EngineKind::Dsp),
        Err(_) => false,
    };
    if pref_ok && degrade_ok {
        s.ok("F1603-校-02-preferNPU降级选路");
    } else {
        s.fail("F1603-校-02-preferNPU降级选路", "降级选路失效");
    }

    // --- 判据元（判据十三）---
    // 元-01：诊断码八码闭集互异 + 文案非空互异 + 版本串锚定。
    let codes = [
        XcCode::HtSchemaUnsupported,
        XcCode::HtAffinityUnsat,
        XcCode::HtBatchCapMiss,
        XcCode::HtRealtimeCapMiss,
        XcCode::HtDeadlinePassed,
        XcCode::HtLabelInvalid,
        XcCode::HtGranZero,
        XcCode::HtModelMissing,
    ];
    let mut uniq = true;
    for i in 0..8 {
        for j in (i + 1)..8 {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let mut det_uniq = true;
    let mut det_nonempty = true;
    let mut details = [""; 8];
    for i in 0..8 {
        details[i] = codes[i].detail();
        if details[i].is_empty() {
            det_nonempty = false;
        }
        for j in (i + 1)..8 {
            if details[i] == details[j] {
                det_uniq = false;
            }
        }
    }
    let meta_ok = uniq && det_uniq && det_nonempty && HET_TASK_SCHEMA == "K03-hettask-v1";
    if meta_ok {
        s.ok("F1603-元-01-八码闭集与文案");
    } else {
        s.fail("F1603-元-01-八码闭集与文案", "诊断码失效");
    }

    // --- 判据元（判据十四）---
    // 元-02：零 panic 面——validate/submit 全路径 Err/回执显性（无 unwrap/
    // expect/panic），除法链除数均为 max(1,·) 或非零常量（编译期可证）。
    // 行为对拍：zero-gran 任务在 submit 前即被 static_valid 拦截，
    // 不达除法点（Err 先行——除法不可达即无除零面）。
    let mut zero_div = base_task();
    zero_div.granularity_ops = 0;
    let mut sched2 = HetScheduler::new(base_candidates());
    let r_zero = sched2.submit(&zero_div, 0);
    let no_panic = matches!(r_zero, Err(XcCode::HtGranZero)) && sched2.dispatched == 0;
    if no_panic {
        s.ok("F1603-元-02-零panic面Err先行");
    } else {
        s.fail("F1603-元-02-零panic面Err先行", "panic 面失效");
    }

    // --- 编排（判据十五）---
    // 编-01：调度面全链路对账——四任务序列：
    // T1 常规 Any → Dispatched{Npu, eta=10×10×1=100}（ops_win=1000/100=10）；
    // T2 批 (9,3) PreferNpu → steps=3、ops_win=1000/100=10 → eta=10×10×3=300；
    // T3 批任务落 DSP（无批）→ NoEngine（no_engine+1）；
    // T4 已过期 → Err(HtDeadlinePassed)（Err 路径计数不动）。
    // 对账：dispatched=2 / missed=0 / no_engine=1 + summary 含版本与候选数。
    let mut sched3 = HetScheduler::new(base_candidates());
    let t1 = base_task();
    let o1 = sched3.submit(&t1, 0);
    let o1_ok = matches!(o1, Ok(DispatchOutcome::Dispatched { engine: EngineKind::Npu, eta_us: 100 }));
    let mut t2 = base_task();
    t2.batch = Some(BatchDim { batch: 9, micro_batch: 3 });
    t2.affinity = Affinity::PreferNpu;
    let o2 = sched3.submit(&t2, 0);
    let o2_ok = matches!(o2, Ok(DispatchOutcome::Dispatched { engine: EngineKind::Npu, eta_us: 300 }));
    let mut t3 = base_task();
    t3.batch = Some(BatchDim { batch: 4, micro_batch: 2 });
    t3.affinity = Affinity::PreferNpu;
    // T3 走仅 DSP 候选（无批能力）→ validate Ok 但 pick None → NoEngine。
    let mut sched4 = HetScheduler::new(alloc::vec![(EngineKind::Dsp, FEAT_REALTIME)]);
    let o3 = sched4.submit(&t3, 0);
    let o3_ok = matches!(o3, Ok(DispatchOutcome::NoEngine)) && sched4.no_engine == 1;
    let mut t4 = base_task();
    t4.deadline_us = Some(100);
    let o4 = sched3.submit(&t4, 500);
    let o4_ok = matches!(o4, Err(XcCode::HtDeadlinePassed));
    let tally_ok = sched3.dispatched == 2 && sched3.missed == 0 && sched3.no_engine == 0;
    let sum3 = sched3.summary();
    let sum_ok = sum3.contains("K03-hettask-v1") && sum3.contains("候选3");
    if o1_ok && o2_ok && o3_ok && o4_ok && tally_ok && sum_ok {
        s.ok("F1603-编-01-调度面全链路对账");
    } else {
        s.fail("F1603-编-01-调度面全链路对账", "全链路失效");
    }

    s
}

/// 亲-02 支撑：以指定亲和构造任务（其余字段取基线）。
fn base_task_with_affinity(a: Affinity) -> HetTaskExt {
    let mut t = base_task();
    t.affinity = a;
    t
}
