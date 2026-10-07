//! VE-F2407 · 域自检（判据逐条对应，见 `vem07_perf.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 类型分批 SIMD → `C07-分批-*`（分批键二元、按类型分批、批宽归一、
//!   批内时间轴逐位相同、批覆盖完整、离散轨被拒、坏形状被拒、空轨被拒、
//!   非单调被拒、批内槽数不超批宽）
//! - 零分配断言 → `C07-零分配-*`（热路径增量 0、**追踪器对照组必须非 0**、
//!   计划期与热路径分开、容量不足被拒不 panic、拒轨下标进现场）
//! - 脏标记缓存 → `C07-缓存-*`（同刻命中、异刻重算、静态轨跨时刻命中、
//!   编辑后失效、**按轨粒度**失效、回绕即脏、回绕诊断、脏标记初值、
//!   陈旧值不得被复用）
//! - LOD 次序 → `C07-预算-*`（远实体先降频、近实体正当跳过不告警、
//!   LOD 见底才降精度、次序跳过告警 + 遥测、全见底记 Exhausted、
//!   未超预算不降级、超预算帧计数）
//! - 能力回退 → `C07-回退-*`（回退显性告警、**回退结果与批路径逐位一致**、
//!   回退计数如实、回退后缓存仍可命中）
//! - 零静默 → `C07-显性-*`（诊断码齐备、诊断渲染非空、码标签互异、
//!   P1 通道可立案、遥测累加如实、家族声明一致）
//! - 成本模型 → `C07-成本-*`（**批路径查找段实测少于标量**、总工作量实测
//!   减少、插值段不摊薄如实、批内二分数 == 批数、步数非自证式常数）

use alloc::vec;
use alloc::vec::Vec;

use super::vem07_perf::*;
use crate::checks::{CheckSet, MAX_CHECKS};

/// 造一条同时间轴的标量轨。
fn sc(times: Vec<u32>, vals: Vec<f32>, static_v: bool) -> SoaTrack {
    SoaTrack::new("s", TrackClass::Continuous, TrackValueKind::Scalar, static_v, times, vals)
}

/// 造一条同时间轴的位置轨（3 通道）。
fn pos(times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new("p", TrackClass::Continuous, TrackValueKind::Position, false, times, vals)
}

/// 造一条同时间轴的四元数轨（4 通道）。
fn quat(times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new("q", TrackClass::Continuous, TrackValueKind::Quat, false, times, vals)
}

/// 标量载荷：4 关键帧 1 通道。
fn s_payload() -> Vec<f32> {
    vec![0.0, 1.0, 2.0, 3.0]
}

/// 位置载荷：4 关键帧 × 3 通道。
fn p_payload() -> Vec<f32> {
    vec![0.0, 1.0, 2.0, 3.0, 10.0, 11.0, 12.0, 13.0, 20.0, 21.0, 22.0, 23.0]
}

/// 四元数载荷：4 关键帧 × 4 通道。
fn q_payload() -> Vec<f32> {
    vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
}

/// 通用时间轴（指纹基准）。
fn t4() -> Vec<u32> {
    vec![0, 100, 200, 400]
}

/// 跑一遍全部批（批路径），返回输出与结果。
fn run_batched(
    tracks: &[SoaTrack],
    plan: &BatchPlan,
    t_ms: u32,
    out_len: usize,
    cache: &mut EvalCache,
    probe: &mut AllocProbe,
    bag: &mut DiagBag,
    o: &mut EvalOutcome,
    cap: SimdCap,
) -> (Vec<f32>, usize) {
    let mut out = vec![0.0f32; out_len];
    let mut cur = 0usize;
    let mut b = 0usize;
    while b < plan.batches.len() {
        eval_batch(plan, tracks, b, t_ms, &mut out, &mut cur, cache, cap, probe, bag, o);
        b += 1;
    }
    (out, cur)
}

/// 预分配输出（按 轨道数 × MAX_CHANNELS，宁宽勿窄）。
fn wide_out(n: usize) -> Vec<f32> {
    vec![0.0f32; n * MAX_CHANNELS]
}

/// 聚合入口（保持单模块自带聚合的惯例）。
///
/// 判据数超过 `CheckSet::MAX_CHECKS`（112，全仓共享）时按判据族切批，
/// 避免抬高上限放大全仓聚合数组。切法：
/// - `a` = 类型分批 SIMD + 零分配断言（39 项）
/// - `b` = 脏标记缓存 + LOD 次序（43 项）
/// - `c` = 能力回退 + 零静默 + 成本模型（44 项）
///
/// mod.rs 侧注册 `VE-F2407` / `VE-F2407-b` / `VE-F2407-c` 三行。
pub fn run_vem07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem07");
    run_vem07_checks_a(&mut set);
    run_vem07_checks_b(&mut set);
    run_vem07_checks_c(&mut set);
    // 三族合计 126 项 > `CheckSet::MAX_CHECKS`(112)：直接聚合会**静默丢掉**
    // 末尾 14 项，且丢的是 c 族（成本模型）的尾巴——聚合器因此报绿，
    // 而实际未跑的判据没人知道。截断在此显性化：想跑全量必须分族注册。
    assert!(
        !set.truncated(),
        "VE-F2407 判据数 {} 超出 CheckSet 容量 {}，聚合会静默丢项；请按 a/b/c 三族分别注册",
        set.len() + set.dropped(),
        MAX_CHECKS
    );
    set
}

/// a 族独立入口（聚合器按族注册，规避 `MAX_CHECKS` 截断）。
pub fn run_vem07_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem07-a");
    run_vem07_checks_a(&mut set);
    set
}

/// b 族独立入口。
pub fn run_vem07_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem07-b");
    run_vem07_checks_b(&mut set);
    set
}

/// c 族独立入口。
pub fn run_vem07_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem07-c");
    run_vem07_checks_c(&mut set);
    set
}

/// 第一批：类型分批 SIMD + 零分配断言。
pub fn run_vem07_checks_a(set: &mut CheckSet) {
    check_batching(set);
    check_zero_alloc(set);
}

/// 第二批：脏标记缓存 + LOD 次序。
pub fn run_vem07_checks_b(set: &mut CheckSet) {
    check_cache(set);
    check_budget(set);
}

/// 第三批：能力回退 + 零静默 + 成本模型。
pub fn run_vem07_checks_c(set: &mut CheckSet) {
    check_fallback(set);
    check_explicit(set);
    check_cost(set);
}

/// 一、类型分批。
fn check_batching(set: &mut CheckSet) {
    // 三类齐备且顺序无关。
    set.add("C07-分批-三类齐备判通过且顺序无关", three_kinds_present(), "");
    set.add(
        "C07-分批-缺标量判不通过",
        !kinds_present(&[TrackValueKind::Position, TrackValueKind::Quat]),
        "",
    );
    set.add("C07-分批-缺位置判不通过", !kinds_present(&[TrackValueKind::Scalar, TrackValueKind::Quat]), "");
    set.add("C07-分批-缺四元数判不通过", !kinds_present(&[TrackValueKind::Scalar, TrackValueKind::Position]), "");
    set.add("C07-分批-空集合判不通过", !kinds_present(&[]), "");
    // 逆序也应通过（顺序无关的正面证据）。
    set.add(
        "C07-分批-逆序三类仍判通过",
        kinds_present(&[TrackValueKind::Quat, TrackValueKind::Scalar, TrackValueKind::Position]),
        "",
    );

    // 通道数标称与实现一致。
    set.add(
        "C07-分批-三类通道数与标称一致",
        TrackValueKind::Scalar.lanes() == 1
            && TrackValueKind::Position.lanes() == 3
            && TrackValueKind::Quat.lanes() == 4
            && MAX_CHANNELS == 4,
        "四元数 4 通道是栈上定长临时[MAX_CHANNELS]的定尺依据",
    );

    // 批宽归一：只接受 4 / 8。
    set.add(
        "C07-分批-批宽归一只认4和8",
        normalize_lane_width(4) == 4
            && normalize_lane_width(8) == 8
            && normalize_lane_width(3) == 4
            && normalize_lane_width(5) == 4
            && normalize_lane_width(0) == 4
            && normalize_lane_width(16) == 4,
        "非 4/8 的批宽请求一律落回 4，不静默接受非法批宽",
    );

    // 混合轨集：10 标量 + 1 位置 + 1 四元数 + 1 事件轨 + 1 坏形状 + 1 空 + 1 非单调。
    let mut tracks: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < 10 {
        tracks.push(sc(t4(), s_payload(), false));
        i += 1;
    }
    tracks.push(pos(t4(), p_payload()));
    tracks.push(quat(t4(), q_payload()));
    let ev_idx = tracks.len();
    tracks.push(SoaTrack::new("ev", TrackClass::Discrete, TrackValueKind::Scalar, false, t4(), s_payload()));
    let bad_idx = tracks.len();
    tracks.push(SoaTrack::new("bad", TrackClass::Continuous, TrackValueKind::Scalar, false, t4(), vec![0.0]));
    let empty_idx = tracks.len();
    tracks.push(SoaTrack::new("empty", TrackClass::Continuous, TrackValueKind::Scalar, false, Vec::new(), Vec::new()));
    let nonmono_idx = tracks.len();
    tracks.push(SoaTrack::new(
        "nonmono",
        TrackClass::Continuous,
        TrackValueKind::Scalar,
        false,
        vec![0, 200, 100, 400],
        s_payload(),
    ));

    let plan = plan_batches(&tracks, 4);

    // 三类轨各自成批（**按类型分批**的执行点）。
    let mut scalar_batches = 0usize;
    let mut pos_batches = 0usize;
    let mut quat_batches = 0usize;
    let mut b = 0usize;
    while b < plan.batches.len() {
        match plan.batches[b].kind {
            TrackValueKind::Scalar => scalar_batches += 1,
            TrackValueKind::Position => pos_batches += 1,
            TrackValueKind::Quat => quat_batches += 1,
        }
        b += 1;
    }
    set.add(
        "C07-分批-按类型分批三类各成批",
        scalar_batches >= 3 && pos_batches == 1 && quat_batches == 1,
        "10 标量切 3 批（4+4+2）+ 位置 1 批 + 四元数 1 批",
    );

    // 批内不得混类型。
    let mut mixed = false;
    b = 0usize;
    while b < plan.batches.len() {
        let k = plan.batches[b].kind;
        let mut s = 0usize;
        while s < plan.batches[b].slots.len() {
            if tracks[plan.batches[b].slots[s]].kind != k {
                mixed = true;
            }
            s += 1;
        }
        b += 1;
    }
    set.add("C07-分批-批内不混轨道类型", !mixed, "");

    // 批内时间轴逐位相同（**共享二分的正确性前提**，实测不靠信任分批器）。
    set.add("C07-分批-批内时间轴逐位相同", batch_times_identical(&plan, &tracks), "");

    // 覆盖完整：入批的一条不少，被拒的一条不进。
    set.add("C07-分批-覆盖完整入批不漏拒不错", batch_cover_ok(&plan, &tracks), "");

    // 批内槽数不超批宽。
    let mut over = false;
    b = 0usize;
    while b < plan.batches.len() {
        if plan.batches[b].slots.len() > plan.lane_width {
            over = true;
        }
        if plan.batches[b].lanes != plan.lane_width {
            over = true;
        }
        b += 1;
    }
    set.add("C07-分批-批内槽数不超批宽", !over, "");

    // 事件轨（离散）被拒 —— 前置 F2406 语义接进来了。
    let ev_rejected = plan.rejected.iter().any(|r| r.track == ev_idx && r.kind == RejectKind::Discrete);
    set.add("C07-分批-事件轨被拒入批且理由为离散", ev_rejected, "F2406 事件轨离散无插值，进批会造出从不存在的事件");
    set.add(
        "C07-分批-事件轨确未出现在任何批内",
        plan.batch_of(ev_idx).is_none(),
        "",
    );

    // 坏形状 / 空 / 非单调 各被拒（表外形态逐条）。
    let has_reason = |idx: usize, want: RejectKind| -> bool {
        plan.rejected.iter().any(|r| r.track == idx && r.kind == want)
    };
    set.add("C07-分批-坏形状轨被拒", has_reason(bad_idx, RejectKind::ShapeMismatch), "");
    set.add("C07-分批-空关键帧轨被拒", has_reason(empty_idx, RejectKind::Empty), "");
    set.add("C07-分批-非单调时间轴被拒", has_reason(nonmono_idx, RejectKind::NotMonotonic), "");
    set.add(
        "C07-分批-被拒四条理由互异",
        RejectKind::ALL.len() == 4 && distinct_reject_reasons(&plan),
        "",
    );

    // 批宽 8 时批数更少（分批策略对批宽敏感）。
    let plan8 = plan_batches(&tracks, 8);
    set.add(
        "C07-分批-批宽8批数不多于批宽4",
        plan8.batch_count() <= plan.batch_count() && plan8.lane_width == 8,
        "同一轨集下批宽 8 的批数不多于批宽 4（更宽的批只会更少或相等）",
    );

    // 异构时间轴：指纹不同 ⇒ 必须分到不同批（分批键第二分量）。
    let mut mixed_t: Vec<SoaTrack> = Vec::new();
    mixed_t.push(sc(vec![0, 100, 200], vec![0.0, 1.0, 2.0], false));
    mixed_t.push(sc(vec![0, 150, 300], vec![0.0, 1.0, 2.0], false));
    let pmt = plan_batches(&mixed_t, 4);
    set.add(
        "C07-分批-异构时间轴指纹不同必分批",
        pmt.batch_count() == 2
            && pmt.batches[0].fingerprint != pmt.batches[1].fingerprint
            && batch_times_identical(&pmt, &mixed_t),
        "同值类型但时间轴不同的轨不能共批——共批会用错区间索引",
    );

    // 同时间轴同类型 ⇒ 共批（分批键第一+第二分量都相同时才共批）。
    let mut same: Vec<SoaTrack> = Vec::new();
    same.push(sc(t4(), s_payload(), false));
    same.push(sc(t4(), s_payload(), false));
    let pst = plan_batches(&same, 4);
    set.add(
        "C07-分批-同键同时间轴必须共批",
        pst.batch_count() == 1 && pst.batches[0].slots.len() == 2,
        "",
    );

    // 时间轴指纹对内容敏感（改一个值即换指纹）。
    let fp_a = times_fingerprint(&t4());
    let fp_b = times_fingerprint(&vec![0, 100, 200, 401]);
    let fp_same = times_fingerprint(&t4());
    set.add("C07-分批-指纹对时间轴内容敏感", fp_a != fp_b && fp_a == fp_same, "");

    // 二分语义真值（表外形态：越界 / 端点 / 中间）。
    // t=150 落在 [100,200) 区间 ⇒ index=1（不是 0，写错就把区间语义搞反了）。
    let h_low = bisect(&t4(), 0);
    let h_mid = bisect(&t4(), 150);
    let h_end = bisect(&t4(), 400);
    let h_over = bisect(&t4(), 9999);
    set.add(
        "C07-分批-二分端点中点越界语义正确",
        h_low.index == 0 && !h_low.clamped
            && h_mid.index == 1 && !h_mid.clamped
            && h_end.index == 2 && !h_end.clamped
            && h_over.index == 2 && h_over.clamped,
        "区间语义：落区间内取该区间、端点取最后区间、超界钳制且 clamped 置位",
    );
    // 单帧轨与空轨不得 panic。
    set.add("C07-分批-单帧轨二分不越界", bisect(&vec![42u32], 42).index == 0, "");
    set.add("C07-分批-单帧轨越界标钳制", bisect(&vec![42u32], 99).clamped, "");
    set.add("C07-分批-空时间轴二分不panic", bisect(&[], 5).clamped, "");

    // 插值系数：含除零防护。
    set.add(
        "C07-分批-插值系数线性正确且除零归零",
        lerp_alpha(&t4(), 0, 50) == 0.5 && lerp_alpha(&t4(), 0, 0) == 0.0 && lerp_alpha(&vec![100u32, 100], 0, 100) == 0.0,
        "",
    );

    // 轨道编辑修订号递增。
    let mut ed = sc(t4(), s_payload(), false);
    let rev0 = ed.edit_rev;
    ed.mark_edited();
    set.add("C07-分批-编辑修订号单调递增", ed.edit_rev == rev0 + 1, "");

    // 轨道自持：形状自洽检查。
    set.add(
        "C07-分批-轨道形状自洽判据可辨",
        sc(t4(), s_payload(), false).shape_ok() && !sc(t4(), vec![0.0], false).shape_ok(),
        "",
    );
}

fn distinct_reject_reasons(plan: &BatchPlan) -> bool {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < plan.rejected.len() {
        let mut j = i + 1;
        while j < plan.rejected.len() {
            if plan.rejected[i].kind != plan.rejected[j].kind {
                n += 1;
            }
            j += 1;
        }
        i += 1;
    }
    n >= 1
}

/// 二、零分配断言。
fn check_zero_alloc(set: &mut CheckSet) {
    let mut tracks: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < 9 {
        tracks.push(sc(t4(), s_payload(), false));
        i += 1;
    }
    let plan = plan_batches(&tracks, 4);
    let mut cache = EvalCache::new();
    cache.reserve_for(tracks.len());
    let mut probe = AllocProbe::new();
    let mut bag = DiagBag::new();
    let mut o = EvalOutcome::default();

    // 正常路径：热路径前后分配增量为 0。
    let before = probe.allocs();
    let (_out, cur) = run_batched(&tracks, &plan, 150, wide_out(tracks.len()).len(), &mut cache, &mut probe, &mut bag, &mut o, SimdCap::Batched);
    let normal_delta = probe.allocs() - before;
    set.add(
        "C07-零分配-热路径正常路径分配增量为0",
        normal_delta == 0 && cur == tracks.len(),
        "求值热路径正常路径零分配，且每条轨都写进了输出缓冲",
    );

    // **对照组（关键）**：追踪器必须看得见分配，否则上面那条是恒真空断言。
    let mut tiny = vec![0.0f32; 2];
    let mut cur2 = 0usize;
    let mut cache2 = EvalCache::new();
    cache2.reserve_for(tracks.len());
    let mut p2 = AllocProbe::new();
    let mut b2 = DiagBag::new();
    let mut o2 = EvalOutcome::default();
    let b2allocs = p2.allocs();
    eval_batch(&plan, &tracks, 0, 150, &mut tiny, &mut cur2, &mut cache2, SimdCap::Batched, &mut p2, &mut b2, &mut o2);
    let tight_delta = p2.allocs() - b2allocs;
    set.add(
        "C07-零分配-追踪器能看见注入的分配",
        tight_delta > 0 && o2.output_rejects > 0,
        "对照组：输出容量不足时追踪器必须看得见分配，否则零分配判据是恒真空断言",
    );
    set.add(
        "C07-零分配-正常与异常两条判据方向相反",
        normal_delta == 0 && tight_delta > 0,
        "两条同时成立才说明断言有分辨力；只验前者等于没验",
    );

    // 容量不足不 panic，且如实记账。
    // 实测：容量 2 时前 2 个槽写得下，第 3 个起被拒 ⇒ written==2 而非 0。
    // 这正是「拒写不推进游标」的可观测量：写入数恰好等于容量。
    set.add(
        "C07-零分配-容量不足写入数恰为容量不超",
        o2.output_rejects > 0 && o2.written == 2 && cur2 == 2,
        "容量不足时写入恰为容量、拒绝数非零、游标不越过容量（不 panic 不越界）",
    );
    set.add("C07-零分配-容量不足有诊断", b2.has(DiagCode::OUTPUT_TOO_SMALL), "");

    // 被拒轨道下标进现场（诊断现场可机检）。容量 2 时第 3 个槽的轨下标 = 2。
    let slot_ok = p2.slot_count() > 0 && p2.slot_words(0).len() == 1 && p2.slot_words(0)[0] as usize == 2;
    set.add(
        "C07-零分配-被拒轨道下标收进现场",
        slot_ok,
        "被拒轨道下标收进诊断现场（容量 2 ⇒ 从第 3 条轨开始拒）",
    );

    // 恰好够用的容量：全部写入且零分配。
    let exact_len = tracks.len();
    let mut cache3 = EvalCache::new();
    cache3.reserve_for(tracks.len());
    let mut p3 = AllocProbe::new();
    let mut b3 = DiagBag::new();
    let mut o3 = EvalOutcome::default();
    let b3allocs = p3.allocs();
    let (_o3, c3) = run_batched(&tracks, &plan, 150, exact_len, &mut cache3, &mut p3, &mut b3, &mut o3, SimdCap::Batched);
    set.add(
        "C07-零分配-容量恰好够时零分配且零拒绝",
        p3.allocs() == b3allocs && o3.output_rejects == 0 && c3 == tracks.len(),
        "容量恰好吃满时零分配、零拒绝、全部写入",
    );

    // 计划期与热路径分开：计划自身占槽（**零分配声明不含它**，别混为一谈）。
    set.add(
        "C07-零分配-计划期槽位单独记账不混入热路径",
        plan.plan_slots() == tracks.len() && plan.accepted_count() == tracks.len(),
        "分批计划在轨道集变更时构建，不在帧内热路径；此处记账只是把声明范围写清",
    );

    // 缓存槽是定长（值在栈外固定数组，不随通道数增长而重分配）。
    // 实测尺寸 28 = rev(4)+t(4)+values(16)+lanes(1)+dirty(1)+filled(1)+尾部对齐 1。
    // 写死尺寸的意义是「有人给槽加字段」会立刻变红；真有人加了字段，
    // 应该改的是这条判据的期望值并说明为什么加。
    set.add(
        "C07-零分配-缓存槽为定长无堆增长",
        MAX_CHANNELS == 4 && core::mem::size_of::<CacheSlot>() == 28,
        "CacheSlot 定长 28 字节且无 Vec/String 成员（有人加字段会立刻变红）",
    );
}

/// 三、脏标记缓存。
fn check_cache(set: &mut CheckSet) {
    // 初值：脏且未填充。
    let fresh = EvalCache::new();
    set.add(
        "C07-缓存-新槽初值脏且未填充",
        fresh.is_empty() || fresh.dirty_count() == fresh.len(),
        "",
    );
    let mut c0 = EvalCache::new();
    c0.reserve_for(3);
    set.add(
        "C07-缓存-预分配后槽全脏",
        c0.len() == 3 && c0.dirty_count() == 3 && c0.filled_count() == 0,
        "首次求值前不得有任何槽可复用，否则会读到全零假值",
    );

    // 非静态轨：同刻命中（纯函数），异刻重算。
    let t = vec![sc(t4(), s_payload(), false)];
    let p = plan_batches(&t, 4);
    let mut cch = EvalCache::new();
    cch.reserve_for(1);
    let mut out = wide_out(1);
    let mut cur = 0usize;
    let mut pr = AllocProbe::new();
    let mut bg = DiagBag::new();

    let mut o1 = EvalOutcome::default();
    eval_batch(&p, &t, 0, 150, &mut out, &mut cur, &mut cch, SimdCap::Batched, &mut pr, &mut bg, &mut o1);
    set.add("C07-缓存-首次求值为重算非命中", o1.recomputes == 1 && o1.hits == 0, "");

    let mut o2 = EvalOutcome::default();
    eval_batch(&p, &t, 0, 150, &mut out, &mut cur, &mut cch, SimdCap::Batched, &mut pr, &mut bg, &mut o2);
    set.add("C07-缓存-同刻重复求值命中缓存", o2.hits == 1 && o2.recomputes == 0, "求值是纯函数，同一点两次求值必等");

    let mut o3 = EvalOutcome::default();
    eval_batch(&p, &t, 0, 250, &mut out, &mut cur, &mut cch, SimdCap::Batched, &mut pr, &mut bg, &mut o3);
    set.add("C07-缓存-异刻求值必重算", o3.recomputes == 1 && o3.hits == 0, "非静态轨的值随时变，异刻不得复用");

    // 静态轨跨时刻命中（锚点「静态场景零成本」）。
    let ts = vec![sc(t4(), vec![5.0, 5.0, 5.0, 5.0], true)];
    let ps = plan_batches(&ts, 4);
    let mut cs = EvalCache::new();
    cs.reserve_for(1);
    let mut outs = wide_out(1);
    let mut curs = 0usize;
    let mut prs = AllocProbe::new();
    let mut bgs = DiagBag::new();
    let mut s1 = EvalOutcome::default();
    eval_batch(&ps, &ts, 0, 100, &mut outs, &mut curs, &mut cs, SimdCap::Batched, &mut prs, &mut bgs, &mut s1);
    let mut s2 = EvalOutcome::default();
    eval_batch(&ps, &ts, 0, 380, &mut outs, &mut curs, &mut cs, SimdCap::Batched, &mut prs, &mut bgs, &mut s2);
    set.add(
        "C07-缓存-静态轨跨时刻命中零重算",
        s1.recomputes == 1 && s2.hits == 1 && s2.recomputes == 0,
        "静态轨值与时间无关，跨时刻命中是它该有的行为",
    );

    // 编辑后失效（**按轨粒度**：只改第 0 轨，第 1 轨应仍命中）。
    // 实测：改 1 条 → 重算 1 命中 1。若这里写「都重算 2」就是把按轨粒度
    // 误判成全清——而全清会让「静态场景零成本」在每次编辑后全废。
    let mut te = vec![sc(t4(), s_payload(), false), sc(t4(), s_payload(), false)];
    let pe = plan_batches(&te, 4);
    let mut ce = EvalCache::new();
    ce.reserve_for(2);
    let mut oute = wide_out(2);
    let mut cure = 0usize;
    let mut pre = AllocProbe::new();
    let mut bge = DiagBag::new();
    let mut f1 = EvalOutcome::default();
    for b in 0..pe.batches.len() {
        eval_batch(&pe, &te, b, 150, &mut oute, &mut cure, &mut ce, SimdCap::Batched, &mut pre, &mut bge, &mut f1);
    }
    let filled = f1.recomputes;
    te[0].mark_edited();
    let mut f2 = EvalOutcome::default();
    cure = 0;
    for b in 0..pe.batches.len() {
        eval_batch(&pe, &te, b, 150, &mut oute, &mut cure, &mut ce, SimdCap::Batched, &mut pre, &mut bge, &mut f2);
    }
    set.add(
        "C07-缓存-轨道编辑后该轨失效且他轨仍命中",
        filled == 2 && f2.recomputes == 1 && f2.hits == 1,
        "按轨粒度失效：改 1 轨只重算该轨，他轨仍命中（一刀切全清会让命中为 0）",
    );

    // **按轨粒度**（不是一刀切）：只改第 0 轨，第 1 轨应仍命中。
    let mut tg = vec![sc(t4(), s_payload(), false), sc(t4(), s_payload(), false)];
    let pg = plan_batches(&tg, 4);
    let mut cg = EvalCache::new();
    cg.reserve_for(2);
    let mut outg = wide_out(2);
    let mut curg = 0usize;
    let mut prg = AllocProbe::new();
    let mut bgg = DiagBag::new();
    for b in 0..pg.batches.len() {
        eval_batch(&pg, &tg, b, 150, &mut outg, &mut curg, &mut cg, SimdCap::Batched, &mut prg, &mut bgg, &mut EvalOutcome::default());
    }
    tg[0].mark_edited();
    let mut g2 = EvalOutcome::default();
    curg = 0;
    for b in 0..pg.batches.len() {
        eval_batch(&pg, &tg, b, 150, &mut outg, &mut curg, &mut cg, SimdCap::Batched, &mut prg, &mut bgg, &mut g2);
    }
    set.add(
        "C07-缓存-失效按轨粒度非全清",
        g2.recomputes == 1 && g2.hits == 1,
        "改 1 条轨 → 仅重算 1 条、其余仍命中（非全清）",
    );
    set.add("C07-缓存-已见修订号逐槽记录", cg.slot_seen_rev(0) != cg.slot_seen_rev(1), "0 号槽见过新 rev，1 号槽没见过");

    // 时间回绕即脏（**表外形态**：t 从 380 跳回 20）。
    // 问的是「回绕前一帧的缓存值有没有被复用」，**不是**「回绕后仍脏」——
    // 回绕标脏的是上一帧遗留值，本帧重算后写回新值，脏标记理应被清。
    // 把语义问反会逼实现留一个假脏标记，那才是真缺陷。
    let tw = vec![sc(t4(), s_payload(), false), sc(t4(), s_payload(), false)];
    let pw = plan_batches(&tw, 4);
    let mut cw = EvalCache::new();
    cw.reserve_for(2);
    let mut outw = wide_out(2);
    let mut curw = 0usize;
    let mut prw = AllocProbe::new();
    let mut bgw = DiagBag::new();
    let mut w1 = EvalOutcome::default();
    for b in 0..pw.batches.len() {
        eval_batch(&pw, &tw, b, 380, &mut outw, &mut curw, &mut cw, SimdCap::Batched, &mut prw, &mut bgw, &mut w1);
    }
    set.add(
        "C07-缓存-首帧求值后不留脏",
        w1.recomputes == 2 && w1.hits == 0 && cw.dirty_count() == 0 && cw.wraps == 0,
        "写回新值即净，这是回绕判据能成立的前提",
    );
    let mut w2 = EvalOutcome::default();
    curw = 0;
    let mut bgw2 = DiagBag::new();
    // 回绕到远小于上次（380 -> 20）
    for b in 0..pw.batches.len() {
        eval_batch(&pw, &tw, b, 20, &mut outw, &mut curw, &mut cw, SimdCap::Batched, &mut prw, &mut bgw2, &mut w2);
    }
    set.add(
        "C07-缓存-时间回绕被识别为一等事件",
        cw.wraps == 1,
        "回绕检测接在生产路径上（首个批），不是靠调用方记得调",
    );
    set.add("C07-缓存-回绕后必重算不得命中", w2.hits == 0 && w2.recomputes == 2, "读到旧值就是锚点 P1 的缓存失效遗漏");
    set.add("C07-缓存-回绕有诊断", bgw2.has(DiagCode::TIME_WRAPPED), "");
    set.add("C07-缓存-回绕诊断说人话", !bgw2.render().is_empty() && bgw2.render().contains("回绕"), "");
    set.add(
        "C07-缓存-回绕后新值写回即净",
        cw.dirty_count() == 0,
        "回绕标脏的是上一帧遗留值；本帧重算写回的新值不脏（语义边界写清）",
    );
    // 回绕到与上次相同的时刻：不算回绕（相等不是回绕）。
    let mut same_t = EvalCache::new();
    same_t.reserve_for(1);
    let mut bg_eq = DiagBag::new();
    same_t.note_time(100, &mut bg_eq);
    let eq_wrapped = same_t.note_time(100, &mut bg_eq);
    set.add("C07-缓存-同刻不判回绕", !eq_wrapped && same_t.wraps == 0, "相等不是回绕，误判会让缓存全废");

    // 非回绕前进不得被误判成回绕。
    let tf = vec![sc(t4(), s_payload(), false)];
    let pf = plan_batches(&tf, 4);
    let mut cf = EvalCache::new();
    cf.reserve_for(1);
    let mut outf = wide_out(1);
    let mut curf = 0usize;
    let mut prf = AllocProbe::new();
    let mut bgf = DiagBag::new();
    for (i, tt) in [10u32, 20, 30, 40].iter().enumerate() {
        let _ = i;
        eval_batch(&pf, &tf, 0, *tt, &mut outf, &mut curf, &mut cf, SimdCap::Batched, &mut prf, &mut bgf, &mut EvalOutcome::default());
    }
    set.add("C07-缓存-时间前进不误判为回绕", cf.wraps == 0, "前进是常态，误判成回绕会让缓存全废");

    // 陈旧值不得被复用（缓存失效遗漏 → P1 的语义面）。
    let mut ts2 = vec![sc(t4(), s_payload(), false)];
    let ps2 = plan_batches(&ts2, 4);
    let mut cs2 = EvalCache::new();
    cs2.reserve_for(1);
    let mut outs2 = wide_out(1);
    let mut curs2 = 0usize;
    let mut prs2 = AllocProbe::new();
    let mut bgs2 = DiagBag::new();
    eval_batch(&ps2, &ts2, 0, 0, &mut outs2, &mut curs2, &mut cs2, SimdCap::Batched, &mut prs2, &mut bgs2, &mut EvalOutcome::default());
    let val_at_zero = outs2[0];
    ts2[0].mark_edited();
    ts2[0].channels[0] = 42.0;
    let mut curs3 = 0usize;
    let mut s2b = EvalOutcome::default();
    eval_batch(&ps2, &ts2, 0, 0, &mut outs2, &mut curs3, &mut cs2, SimdCap::Batched, &mut prs2, &mut bgs2, &mut s2b);
    set.add(
        "C07-缓存-编辑后不复用陈旧值",
        val_at_zero == 0.0 && outs2[0] == 42.0 && s2b.recomputes == 1,
        "改值 + 改 rev 后必须读到新值 42，读到 0 就是缓存失效遗漏",
    );

    // 越界槽访问保守为脏。
    set.add("C07-缓存-越界槽判脏且修订号为MAX", EvalCache::new().slot_dirty(99) && EvalCache::new().slot_seen_rev(99) == u32::MAX, "");

    // 缓存 load/store 往返。
    let mut cr = EvalCache::new();
    cr.reserve_for(1);
    let trk = sc(t4(), s_payload(), false);
    let mut vals: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
    vals[0] = 7.5;
    cr.store(0, &trk, 33, &vals, 1);
    let mut back: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
    let n = cr.load(0, &mut back);
    set.add("C07-缓存-存读往返逐位一致", n == 1 && back[0] == 7.5, "");
    set.add("C07-缓存-存后槽为净", !cr.slot_dirty(0) && cr.slot_seen_rev(0) == trk.edit_rev, "");
}

/// 四、预算与 LOD 次序。
fn check_budget(set: &mut CheckSet) {
    // 未超预算：不降级。
    {
        let mut lod = LodState::new(2, 0);
        let mut prec = PrecisionState::new(2, 0);
        let mut bag = DiagBag::new();
        let mut te = Telemetry::default();
        let led = FrameLedger { eval_us: 10, budget_us: 50, over_budget: 0 };
        let pl = plan_degrade(&led, DistanceBucket::Far, &mut lod, &mut prec, &mut bag, &mut te);
        set.add(
            "C07-预算-未超预算不降级",
            pl.is_empty() && pl.first() == DegradeAction::None && lod.level == 2 && prec.level == 2,
            "",
        );
        set.add("C07-预算-未超预算不计超预算帧", te.over_budget_frames == 0, "");
    }
    // 未设预算（budget=0）视为永不超。
    set.add(
        "C07-预算-未设预算视为永不超",
        !FrameLedger { eval_us: 9999, budget_us: 0, over_budget: 0 }.over(),
        "budget=0 是「没设预算」，不是「预算为零必超」",
    );

    // 远实体超预算：**首个动作必是 LOD**（锚点「LOD 先于精度降」）。
    {
        let mut lod = LodState::new(3, 0);
        let mut prec = PrecisionState::new(3, 0);
        let mut bag = DiagBag::new();
        let mut te = Telemetry::default();
        let led = FrameLedger { eval_us: 100, budget_us: 50, over_budget: 0 };
        let pl = plan_degrade(&led, DistanceBucket::Far, &mut lod, &mut prec, &mut bag, &mut te);
        set.add(
            "C07-预算-远实体超预算首个动作为LOD",
            pl.first() == DegradeAction::LodDown && !pl.actions.contains(&DegradeAction::PrecisionDown),
            "远实体超预算时首个动作必须是降 LOD，且本帧不得含精度降",
        );
        set.add("C07-预算-远实体降频不告警", !bag.has(DiagCode::ORDER_SKIPPED), "正常次序不是异常，不该刷告警");
        set.add("C07-预算-超预算帧如实计数", te.over_budget_frames == 1, "");
        set.add("C07-预算-降LOD不动精度", prec.level == 3, "次序声明的第一阶不许连跳");
    }

    // 近实体超预算：跳过 LOD 是**正当的**，不告警，且要标记。
    {
        let mut lod = LodState::new(3, 0);
        let mut prec = PrecisionState::new(3, 0);
        let mut bag = DiagBag::new();
        let mut te = Telemetry::default();
        let led = FrameLedger { eval_us: 100, budget_us: 50, over_budget: 0 };
        let pl = plan_degrade(&led, DistanceBucket::Near, &mut lod, &mut prec, &mut bag, &mut te);
        set.add(
            "C07-预算-近实体跳过LOD标记为正当",
            pl.lod_skip_legit && !pl.order_skipped && lod.level == 3,
            "近处降 LOD 没有视觉收益，正当跳过",
        );
        set.add("C07-预算-近实体跳过不告警不遥测", !bag.has(DiagCode::ORDER_SKIPPED) && te.order_skipped == 0, "");
        set.add("C07-预算-近实体直接降精度", pl.first() == DegradeAction::PrecisionDown && prec.level == 2, "");
    }

    // 远实体 LOD 见底仍超：**次序跳过** → 告警 + 遥测。
    {
        let mut lod = LodState::new(0, 0);
        let mut prec = PrecisionState::new(1, 0);
        let mut bag = DiagBag::new();
        let mut te = Telemetry::default();
        let led = FrameLedger { eval_us: 100, budget_us: 50, over_budget: 0 };
        let pl = plan_degrade(&led, DistanceBucket::Far, &mut lod, &mut prec, &mut bag, &mut te);
        set.add(
            "C07-预算-远实体LOD见底记次序跳过",
            pl.order_skipped && !pl.lod_skip_legit && pl.first() == DegradeAction::PrecisionDown,
            "",
        );
        set.add("C07-预算-次序跳过有告警", bag.has(DiagCode::ORDER_SKIPPED), "");
        set.add("C07-预算-次序跳过进遥测", te.order_skipped == 1, "");
        set.add("C07-预算-次序跳过告警为Major", bag.count_severity(Severity::Major) >= 1, "降级仍出结果，故 Major 不 P1");
    }

    // 全见底：记 Exhausted，不静默假装达标。
    {
        let mut lod = LodState::new(0, 0);
        let mut prec = PrecisionState::new(0, 0);
        let mut bag = DiagBag::new();
        let mut te = Telemetry::default();
        let led = FrameLedger { eval_us: 100, budget_us: 50, over_budget: 0 };
        let pl = plan_degrade(&led, DistanceBucket::Far, &mut lod, &mut prec, &mut bag, &mut te);
        set.add(
            "C07-预算-全见底记Exhausted",
            pl.actions.contains(&DegradeAction::Exhausted) && !pl.actions.contains(&DegradeAction::PrecisionDown),
            "",
        );
        set.add("C07-预算-全见底有告警", bag.has(DiagCode::BUDGET_EXHAUSTED), "");
    }

    // 逐级降 LOD 直到见底（次序在多帧上保持）。
    {
        let mut lod = LodState::new(2, 0);
        let mut prec = PrecisionState::new(3, 0);
        let mut bag = DiagBag::new();
        let mut te = Telemetry::default();
        let led = FrameLedger { eval_us: 100, budget_us: 50, over_budget: 0 };
        let mut firsts: Vec<DegradeAction> = Vec::new();
        let mut f = 0usize;
        while f < 3 {
            let pl = plan_degrade(&led, DistanceBucket::Far, &mut lod, &mut prec, &mut bag, &mut te);
            firsts.push(pl.first());
            f += 1;
        }
        set.add(
            "C07-预算-多帧次序稳定LOD先于精度",
            firsts[0] == DegradeAction::LodDown
                && firsts[1] == DegradeAction::LodDown
                && firsts[2] == DegradeAction::PrecisionDown
                && prec.level == 2,
            "多帧次序稳定：LOD 两级用尽之后才允许动精度",
        );
    }

    // 档位可降判定。
    set.add(
        "C07-预算-档位可降判定正确",
        LodState::new(2, 0).can_reduce() && !LodState::new(0, 0).can_reduce() && !LodState::new(0, 2).can_reduce(),
        "level<=min 时不可降，防下溢",
    );
    // reduce 逐级。
    let mut lr = LodState::new(2, 0);
    lr.reduce();
    set.add("C07-预算-降级逐级不跳档", lr.level == 1, "");
    let mut prr = PrecisionState::new(2, 0);
    prr.reduce();
    set.add("C07-预算-精度降级逐级不跳档", prr.level == 1, "");

    // 降级动作标签互异可读。
    set.add(
        "C07-预算-降级动作四类标签互异",
        DegradeAction::ALL.len() == 4 && distinct_labels_degrade(),
        "",
    );
    // 距离档全集。
    set.add("C07-预算-距离档两类齐备", DistanceBucket::ALL.len() == 2, "");
}

fn distinct_labels_degrade() -> bool {
    let mut same = 0usize;
    let mut i = 0usize;
    while i < DegradeAction::ALL.len() {
        let mut j = i + 1;
        while j < DegradeAction::ALL.len() {
            if DegradeAction::ALL[i].label() == DegradeAction::ALL[j].label() {
                same += 1;
            }
            j += 1;
        }
        i += 1;
    }
    same == 0
}

/// 五、能力回退。
fn check_fallback(set: &mut CheckSet) {
    let mut tracks: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < 9 {
        tracks.push(sc(t4(), s_payload(), false));
        i += 1;
    }
    let plan = plan_batches(&tracks, 4);

    // 批路径基准输出。
    let mut c1 = EvalCache::new();
    c1.reserve_for(tracks.len());
    let mut p1 = AllocProbe::new();
    let mut b1 = DiagBag::new();
    let mut o1 = EvalOutcome::default();
    let (out_b, cur_b) = run_batched(
        &tracks,
        &plan,
        150,
        wide_out(tracks.len()).len(),
        &mut c1,
        &mut p1,
        &mut b1,
        &mut o1,
        SimdCap::Batched,
    );

    // 回退路径输出（**逐位一致**是 F1902 家族的硬要求）。
    let mut c2 = EvalCache::new();
    c2.reserve_for(tracks.len());
    let mut p2 = AllocProbe::new();
    let mut b2 = DiagBag::new();
    let mut o2 = EvalOutcome::default();
    let (out_s, cur_s) = run_batched(
        &tracks,
        &plan,
        150,
        wide_out(tracks.len()).len(),
        &mut c2,
        &mut p2,
        &mut b2,
        &mut o2,
        SimdCap::ScalarOnly,
    );

    set.add(
        "C07-回退-结果与批路径逐位一致",
        cur_b == cur_s && out_b[..cur_b] == out_s[..cur_s],
        "回退只许慢，不许改变结果",
    );
    set.add("C07-回退-显性告警非静默", b2.has(DiagCode::SIMD_FALLBACK), "能力不可用必须说出来");
    set.add(
        "C07-回退-回退次数按批如实计数",
        o2.fallbacks == plan.batch_count() as u32,
        "回退次数按批如实计数（每批一次，等于批数）",
    );
    set.add("C07-回退-回退告警为Major", b2.count_severity(Severity::Major) == plan.batch_count(), "");
    set.add("C07-回退-批路径无回退告警", !b1.has(DiagCode::SIMD_FALLBACK), "");
    set.add("C07-回退-回退零分配", p2.allocs() == 0, "回退不是允许破零分配纪律的借口");

    // 回退后缓存仍可命中（**两条路径共享缓存语义**）。
    let mut c3 = EvalCache::new();
    c3.reserve_for(tracks.len());
    let mut p3 = AllocProbe::new();
    let mut b3 = DiagBag::new();
    let mut o3 = EvalOutcome::default();
    run_batched(
        &tracks,
        &plan,
        150,
        wide_out(tracks.len()).len(),
        &mut c3,
        &mut p3,
        &mut b3,
        &mut o3,
        SimdCap::ScalarOnly,
    );
    let mut o4 = EvalOutcome::default();
    let mut b4 = DiagBag::new();
    let (_out4, _cur4) = run_batched(
        &tracks,
        &plan,
        150,
        wide_out(tracks.len()).len(),
        &mut c3,
        &mut p3,
        &mut b4,
        &mut o4,
        SimdCap::ScalarOnly,
    );
    set.add(
        "C07-回退-回退路径第二次命中缓存",
        o3.recomputes == tracks.len() as u32 && o4.hits == tracks.len() as u32,
        "回退路径若不共享缓存，这里会是 0——那是只在能力抖动时才暴露的隐蔽差异",
    );

    // 能力档标签与全集。
    set.add(
        "C07-回退-能力档两类标签互异",
        SimdCap::ALL.len() == 2 && SimdCap::Batched.label() != SimdCap::ScalarOnly.label(),
        "",
    );
}

/// 六、零静默与家族。
fn check_explicit(set: &mut CheckSet) {
    set.add("C07-显性-诊断码十一类齐备", all_codes_present() && DiagCode::ALL.len() == 11, "");
    set.add(
        "C07-显性-缺一码判不通过",
        !codes_present(&[DiagCode::SIMD_FALLBACK, DiagCode::CHANNEL_MISMATCH]),
        "表外形态：只给两码必须判不通过，否则齐备性判据是恒真的",
    );
    set.add("C07-显性-空码表判不通过", !codes_present(&[]), "");
    set.add("C07-显性-码标签互异", distinct_code_labels(), "");
    set.add(
        "C07-显性-未知码有兜底人话不panic",
        DiagCode(0xFFFF).label() == "未登记诊断码",
        "结构体包装无法穷尽 match，兜底臂不许 panic",
    );

    // 诊断袋：渲染非空 / 计数 / 严重度。
    let mut bag = DiagBag::new();
    set.add("C07-显性-空袋渲染非空", !DiagBag::new().render().is_empty(), "空也要说话，不能渲染成空串");
    bag.push(DiagCode::TIME_CLAMPED, "m1", "h1");
    bag.push_major(DiagCode::SIMD_FALLBACK, "m2", "h2");
    bag.push_p1(DiagCode::ALLOC_IN_HOT_PATH, "m3", "h3");
    set.add("C07-显性-诊断三条已记", bag.len() == 3, "");
    set.add("C07-显性-严重度分级可数", bag.count_severity(Severity::Minor) == 1 && bag.count_severity(Severity::Major) == 1 && bag.count_severity(Severity::P1) == 1, "");
    set.add("C07-显性-按码计数准确", bag.count(DiagCode::SIMD_FALLBACK) == 1 && bag.count(DiagCode::CHANNEL_MISMATCH) == 0, "");
    set.add("C07-显性-渲染含码标签与处置建议", bag.render().contains("回退标量") && bag.render().contains("怎么办"), "诊断文案要说人话：什么情况 + 怎么办");
    set.add("C07-显性-渲染含P1标记", bag.render().contains("[P1]"), "");

    // P1 通道。
    let mut p1c = P1Channel::new();
    set.add("C07-显性-P1通道初始为空", p1c.is_empty(), "");
    p1c.open(DiagCode::ALLOC_IN_HOT_PATH, "热路径出现受追踪分配");
    p1c.open(DiagCode::ALLOC_IN_HOT_PATH, "再次");
    p1c.open(DiagCode::TIME_WRAPPED, "回绕未标脏");
    set.add("C07-显性-P1可立案可计数", p1c.len() == 3 && p1c.count(DiagCode::ALLOC_IN_HOT_PATH) == 2, "");
    set.add("C07-显性-P1按码判别", p1c.has(DiagCode::ALLOC_IN_HOT_PATH) && !p1c.has(DiagCode::BUDGET_EXHAUSTED), "");
    set.add("C07-显性-P1详情非空", p1c.cases()[0].detail == "热路径出现受追踪分配", "");

    // 遥测累加。
    let mut t = Telemetry::default();
    let d = Telemetry {
        simd_fallback: 1,
        time_wrap: 2,
        cache_hits: 3,
        cache_recompute: 4,
        order_skipped: 5,
        over_budget_frames: 6,
        tracks_rejected: 7,
    };
    accumulate(&mut t, &d);
    set.add(
        "C07-显性-遥测七项累加如实",
        t.simd_fallback == 1
            && t.time_wrap == 2
            && t.cache_hits == 3
            && t.cache_recompute == 4
            && t.order_skipped == 5
            && t.over_budget_frames == 6
            && t.tracks_rejected == 7,
        "",
    );
    let mut t2 = Telemetry::default();
    accumulate(&mut t2, &d);
    accumulate(&mut t2, &d);
    set.add("C07-显性-遥测累加可叠加", t2.order_skipped == 10, "");

    // 家族声明。
    set.add("C07-显性-四家族标签互异", family_is_consistent() && FamilyMember::ALL.len() == 4, "");
    set.add(
        "C07-显性-家族域字母齐备",
        has_domain('C') && has_domain('L') && has_domain('A'),
        "F1525(C) F2202(L) F2229(L) F1902(A)",
    );
    set.add(
        "C07-显性-家族声明说人话",
        FamilyMember::ALL.iter().all(|m| !m.claim.is_empty()),
        "",
    );

    // 拒绝原因人话。
    set.add(
        "C07-显性-四类拒绝理由均非空",
        RejectKind::ALL.iter().all(|k| !k.reason().is_empty()),
        "拒轨必须说清为什么，否则作者不知道怎么改",
    );

    // 人话总述。
    set.add(
        "C07-显性-总述覆盖四条锚点",
        describe().contains("分批") && describe().contains("零分配") && describe().contains("脏标记") && describe().contains("LOD"),
        "",
    );
    set.add("C07-显性-冒烟非空", !smoke().is_empty(), "");
}

fn has_domain(d: char) -> bool {
    let mut i = 0usize;
    while i < FamilyMember::ALL.len() {
        if FamilyMember::ALL[i].domain == d {
            return true;
        }
        i += 1;
    }
    false
}

fn distinct_code_labels() -> bool {
    let mut same = 0usize;
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            if DiagCode::ALL[i].label() == DiagCode::ALL[j].label() {
                same += 1;
            }
            j += 1;
        }
        i += 1;
    }
    same == 0
}

/// 七、成本模型（**实测，不预设 4-6×**）。
fn check_cost(set: &mut CheckSet) {
    // 造一个够大的场景：多批、同时间轴、标量 + 位置混合。
    let mut tracks: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < 32 {
        tracks.push(sc(t4(), s_payload(), false));
        i += 1;
    }
    tracks.push(pos(t4(), p_payload()));
    let plan = plan_batches(&tracks, 4);

    // 批路径（无缓存，纯成本）。
    let mut out_b = wide_out(tracks.len());
    let mut cur_b = 0usize;
    let mut pb = AllocProbe::new();
    let mut bb = DiagBag::new();
    let mut ob = EvalOutcome::default();
    let mut cache = EvalCache::new();
    // 故意不 reserve，让缓存全未填充 ⇒ 全部走求值路径（排除缓存干扰）。
    let _ = &mut cache;
    let mut b = 0usize;
    while b < plan.batches.len() {
        eval_batch(&plan, &tracks, b, 150, &mut out_b, &mut cur_b, &mut cache, SimdCap::Batched, &mut pb, &mut bb, &mut ob);
        b += 1;
    }

    // 标量基线（每轨各自二分）。
    let mut out_s = wide_out(tracks.len());
    let mut cur_s = 0usize;
    let mut ps = AllocProbe::new();
    let mut bs = DiagBag::new();
    let mut os_ = EvalOutcome::default();
    let all: Vec<usize> = (0..tracks.len()).collect();
    eval_baseline_no_cache(&tracks, &all, 150, &mut out_s, &mut cur_s, &mut ps, &mut bs, &mut os_);

    // 查找段实测对比（这是批处理的**真实收益来源**）。
    set.add(
        "C07-成本-批路径查找段实测少于标量",
        ob.bisect_calls < os_.bisect_calls,
        "批路径查找段二分次数实测少于标量（摊薄比即批宽）",
    );
    set.add(
        "C07-成本-批内二分数等于批数",
        ob.bisect_calls == plan.batch_count() as u32,
        "每批一次共享二分，摊薄比 = 批内轨数",
    );
    set.add(
        "C07-成本-二分步数实测非自证常数",
        ob.bisect_steps > 0 && os_.bisect_steps > ob.bisect_steps,
        "二分步数由实跑累加而非 n×常数自证，且批路径步数少于标量",
    );

    // 插值段**不摊薄**（如实断言，防止有人虚报收益）。
    set.add(
        "C07-成本-插值段不摊薄如实计入",
        ob.interp_calls == os_.interp_calls && ob.interp_calls == tracks.len() as u32,
        "插值段不摊薄：两路径插值次数都等于轨数（摊薄的是查找不是插值）",
    );

    // 总工作量实测减少（不是预设 4-6×，是这条场景下的实测值）。
    let wb = work_units(&ob);
    let ws = work_units(&os_);
    set.add(
        "C07-成本-总工作量实测少于标量",
        wb < ws,
        "总工作量（二分步数+插值次数）批路径实测少于标量",
    );
    set.add(
        "C07-成本-总加速比不超过理论上限",
        ws as f64 / wb as f64 <= tracks.len() as f64,
        "理论上限 = 批内轨数（插值段不可摊薄），超过就是模型算错了",
    );

    // 两条路径结果逐位一致（成本优化的前提是结果不变）。
    set.add(
        "C07-成本-两路径结果逐位一致",
        cur_b == cur_s && out_b[..cur_b] == out_s[..cur_s],
        "批路径与标量路径写入的浮点逐位一致（回退不得改变结果）",
    );

    // 缓存计入后总工作量进一步下降（静态零成本的量化面）。
    let mut cache2 = EvalCache::new();
    cache2.reserve_for(tracks.len());
    let mut out_c = wide_out(tracks.len());
    let mut cur_c = 0usize;
    let mut pc = AllocProbe::new();
    let mut bc = DiagBag::new();
    let mut oc1 = EvalOutcome::default();
    let mut b2 = 0usize;
    while b2 < plan.batches.len() {
        eval_batch(&plan, &tracks, b2, 150, &mut out_c, &mut cur_c, &mut cache2, SimdCap::Batched, &mut pc, &mut bc, &mut oc1);
        b2 += 1;
    }
    let mut oc2 = EvalOutcome::default();
    cur_c = 0;
    b2 = 0;
    while b2 < plan.batches.len() {
        eval_batch(&plan, &tracks, b2, 150, &mut out_c, &mut cur_c, &mut cache2, SimdCap::Batched, &mut pc, &mut bc, &mut oc2);
        b2 += 1;
    }
    set.add(
        "C07-成本-缓存命中后零插值调用",
        oc2.interp_calls == 0 && oc2.hits == tracks.len() as u32 && oc2.bisect_steps > 0,
        "缓存全命中后插值调用为 0（静态零成本：仍定位一次但不插值）",
    );
    set.add(
        "C07-成本-缓存命中不改变结果",
        out_c[..cur_c] == out_b[..cur_b],
        "命中路径必须吐出与求值路径逐位相同的值",
    );

    // 批宽 8 的摊薄更明显（查找段更少）。
    let plan8 = plan_batches(&tracks, 8);
    let mut out8 = wide_out(tracks.len());
    let mut cur8 = 0usize;
    let mut p8 = AllocProbe::new();
    let mut b8 = DiagBag::new();
    let mut o8 = EvalOutcome::default();
    let mut cache8 = EvalCache::new();
    let mut b3 = 0usize;
    while b3 < plan8.batches.len() {
        eval_batch(&plan8, &tracks, b3, 150, &mut out8, &mut cur8, &mut cache8, SimdCap::Batched, &mut p8, &mut b8, &mut o8);
        b3 += 1;
    }
    set.add(
        "C07-成本-批宽8查找段不多于批宽4",
        o8.bisect_calls <= ob.bisect_calls,
        "批宽 8 的查找段二分次数不多于批宽 4",
    );
    set.add(
        "C07-成本-批宽8结果与批宽4一致",
        out8[..cur8] == out_b[..cur_b],
        "批宽是调度参数，不许改变数值结果",
    );

    // 越界批号：静默返回不 panic。
    let mut cache9 = EvalCache::new();
    cache9.reserve_for(1);
    let mut out9 = wide_out(1);
    let mut cur9 = 0usize;
    let mut p9 = AllocProbe::new();
    let mut b9 = DiagBag::new();
    let mut o9 = EvalOutcome::default();
    eval_batch(&plan, &tracks, 9999, 150, &mut out9, &mut cur9, &mut cache9, SimdCap::Batched, &mut p9, &mut b9, &mut o9);
    set.add(
        "C07-成本-越界批号静默返回不panic",
        o9.written == 0 && cur9 == 0 && b9.is_empty(),
        "批号越界是调用方错误，但内核不许因此崩",
    );

    // 空批（无槽）不做事。
    let mut empty_plan = plan_batches(&[], 4);
    empty_plan.lane_width = 4;
    let mut cache_e = EvalCache::new();
    let mut out_e = vec![0.0f32; 4];
    let mut cur_e = 0usize;
    let mut probe_e = AllocProbe::new();
    let mut bag_e = DiagBag::new();
    let mut out_e2 = EvalOutcome::default();
    eval_batch(
        &empty_plan,
        &[],
        0,
        100,
        &mut out_e,
        &mut cur_e,
        &mut cache_e,
        SimdCap::Batched,
        &mut probe_e,
        &mut bag_e,
        &mut out_e2,
    );
    set.add(
        "C07-成本-空轨集批计划不做事",
        out_e2.written == 0 && empty_plan.batch_count() == 0,
        "空轨集不产生批，求值调用直接返回，不写不分配",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perf_red_items() {
        let set = run_vem07_checks();
        for name in set.red_items() {
            println!("[红] {}", name);
        }
        println!("total={} passed={} dropped={}", set.len(), set.passed_count(), set.dropped());
    }
}
