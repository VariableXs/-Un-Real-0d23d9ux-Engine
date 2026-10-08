//! VE-F2408 · 域自检（判据逐条对应，见 `vem08_debug.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三负载 → `C08-负载-*`（曲线拾取语义/值流环形语义/热力三冗余）
//! - 按需拾取 → `C08-拾取-*`（端点钉死、抽稀比、窗边界、洪水钳制）
//! - 协议家族 → `C08-协议-*`（工作单元实测、序号水位线两语义、超预算）
//! - M 段注册 → `C08-信封-*`（四类型注册、漂移拦截、解拦截后恢复）
//! - 零静默 → `C08-显性-*`（诊断码齐备、标签互异、P1 可查、剔除失败立案）
//! - 剔除零成本 → `C08-剔除-*`（**双向**：Debug 必须递增作对照，
//!   Release 连强行尝试都不递增）
//!
//! **弱门禁自律**：本文件每一条「恒真型」判据都配了**对照组或变异方向**，
//! 判据侧自己举反例、自己重算，不问被测函数「你返回 true 吗」。

use alloc::vec;
use alloc::vec::Vec;

use super::vem03_interp::Interp;
use super::vem07_perf::{SoaTrack, Telemetry, TrackClass, TrackValueKind};
use super::vem08_debug::*;
use crate::checks::{CheckSet, MAX_CHECKS};

/// 造一条标量轨（1 通道）。
fn sc(times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new("s", TrackClass::Continuous, TrackValueKind::Scalar, false, times, vals)
}

/// 造一条位置轨（3 通道）。
fn pos(times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    SoaTrack::new("p", TrackClass::Continuous, TrackValueKind::Position, false, times, vals)
}

/// 通用时间轴（8 关键帧）。
fn t8() -> Vec<u32> {
    vec![0, 100, 200, 300, 400, 500, 600, 700]
}

/// 通用载荷（8 关键帧 × 1 通道）。
fn s8() -> Vec<f32> {
    vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
}

/// 空诊断袋。
fn bag() -> DiagBag {
    DiagBag::new()
}

/// 遥测（指定缓存命中/重算）。
fn tele(hits: u32, rec: u32) -> Telemetry {
    Telemetry { cache_hits: hits, cache_recompute: rec, ..Telemetry::default() }
}

/// 判据数超过 `CheckSet::MAX_CHECKS`（112，全仓共享）时按判据族切批：
/// - `a` = 三负载 + 按需拾取（38 项）
/// - `b` = 协议家族 + M 段信封（34 项）
/// - `c` = 零静默 + 剔除零成本 + 无障碍（32 项）
///
/// mod.rs 侧注册 `VE-F2408-a` / `VE-F2408-b` / `VE-F2408-c` 三行。
pub fn run_vem08_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem08");
    run_vem08_checks_a(&mut set);
    run_vem08_checks_b(&mut set);
    run_vem08_checks_c(&mut set);
    // 三族合计 > `CheckSet::MAX_CHECKS`(112)：直接聚合会**静默丢掉**末尾项，
    // 而丢的是 c 族（剔除/无障碍）的尾巴——聚合器因此报绿，未跑的判据
    // 没人知道。截断在此显性化：想跑全量必须分族注册。
    assert!(
        !set.truncated(),
        "VE-F2408 判据数 {} 超出 CheckSet 容量 {}，聚合会静默丢项；请按 a/b/c 三族分别注册",
        set.len() + set.dropped(),
        MAX_CHECKS
    );
    set
}

/// a 族独立入口。
pub fn run_vem08_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem08-a");
    run_vem08_checks_a(&mut set);
    set
}

/// b 族独立入口。
pub fn run_vem08_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem08-b");
    run_vem08_checks_b(&mut set);
    set
}

/// c 族独立入口。
pub fn run_vem08_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem08-c");
    run_vem08_checks_c(&mut set);
    set
}

/// 第一批：三负载 + 按需拾取。
pub fn run_vem08_checks_a(set: &mut CheckSet) {
    check_payloads(set);
    check_pick(set);
}

/// 第二批：协议家族 + M 段信封。
pub fn run_vem08_checks_b(set: &mut CheckSet) {
    check_protocol(set);
    check_envelope(set);
}

/// 第三批：零静默 + 剔除零成本 + 无障碍。
pub fn run_vem08_checks_c(set: &mut CheckSet) {
    check_explicit(set);
    check_strip(set);
    check_a11y(set);
}

// ---------------------------------------------------------------------------
// 一、三负载
// ---------------------------------------------------------------------------

fn check_payloads(set: &mut CheckSet) {
    // 曲线负载：全窗拾取逐点还原原值（**判据侧独立重算**，不信被测函数）。
    let track = sc(t8(), s8());
    let mut b = bag();
    let p = pick_curve(&track, 0, 0, 700, 64, Interp::Linear, &mut b);
    let mut values_ok = p.points.len() == 8;
    let mut i = 0usize;
    while i < p.points.len() {
        if p.points[i].values[0] != s8()[i] || p.points[i].t_ms != t8()[i] {
            values_ok = false;
        }
        i += 1;
    }
    set.add("C08-负载-曲线全窗逐点还原原值", values_ok, "");
    set.add("C08-负载-曲线全窗未抽稀且记账相符", !p.decimated && p.total_keys == 8, "");
    set.add("C08-负载-曲线插值器标记透传F2403", p.points[0].interp == Interp::Linear, "");
    set.add(
        "C08-负载-曲线通道数随值类型",
        p.points[0].lanes as usize == TrackValueKind::Scalar.lanes(),
        "",
    );

    // 位置轨 3 通道：lanes 必须是 3（**防按标量 1 通道取**）。
    let ptrack = pos(vec![0, 100, 200], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    let mut b2 = bag();
    let pp = pick_curve(&ptrack, 0, 0, 200, 64, Interp::Linear, &mut b2);
    set.add("C08-负载-位置轨三通道", pp.points[0].lanes == 3, "");
    // 三通道值逐位还原（独立重算索引 `k*3+c`）。
    set.add(
        "C08-负载-位置轨通道索引正确",
        pp.points[1].values[0] == 4.0 && pp.points[1].values[1] == 5.0 && pp.points[1].values[2] == 6.0,
        "",
    );

    // 值流：定容环三态（未满/恰好满/覆盖）。
    let mut st = ValueStream::with_capacity(3);
    let mut ok_first = true;
    let mut k = 0u32;
    while k < 3 {
        if !st.push(sample(k, k as f32), &mut bag()) {
            ok_first = false;
        }
        k += 1;
    }
    set.add("C08-负载-值流未满不记覆盖", ok_first && st.overwritten() == 0, "");
    let mut b3 = bag();
    let over = st.push(sample(3, 9.0), &mut b3);
    set.add("C08-负载-值流覆盖如实记账", !over && st.overwritten() == 1, "");
    set.add("C08-负载-值流覆盖有诊断", b3.has(DiagCode::STREAM_OVERWRITTEN), "");
    set.add("C08-负载-值流容量恒定不增长", st.capacity() == 3 && st.len() == 3, "");
    set.add("C08-负载-值流最新样本是最后推入", st.latest().map(|s| s.t_ms) == Some(3), "");

    // 值流窗口计数：半开区间 `(now-100, now]`。
    let mut st2 = ValueStream::with_capacity(8);
    let mut m = 0u32;
    while m < 5 {
        let _ = st2.push(sample(m * 50, m as f32), &mut bag());
        m += 1;
    }
    // 半开区间 `(now-100, now]`：样本落在 0/50/100/150/200，
    // 命中的是 150 与 200 共 2 个——下界 100 **不在**区间内（半开不含下界）。
    set.add("C08-负载-值流窗口半开区间计数", st2.window_count(200, 100) == 2, "");
    // 夹逼对：窗口右移一位，下界跟着移一位（证明是区间语义不是常数 2）。
    set.add("C08-负载-值流窗口随下界移动", st2.window_count(200, 200) == 4, "");
    // 零宽窗 `(200,200]` 本身就是空区间 → 0（不是 1）。
    set.add("C08-负载-值流零宽窗为空区间", st2.window_count(200, 0) == 0, "");
    // 窗宽大于 now：`now - window_ms` 会下溢。此处必须**靠守卫短路**返回 0，
    // 而不是 panic 或回绕出巨大窗口（回绕会让计数变成「全命中」的假数）。
    set.add("C08-负载-值流窗宽超now不下溢", st2.window_count(200, 250) == 0, "");
    // 夹逼对：窗宽跨过下界时刻，命中数恰好 +1（证明是区间语义不是阶梯）。
    set.add("C08-负载-值流窗宽夹逼对", st2.window_count(200, 100) + 1 == st2.window_count(200, 101), "");

    // 空流 latest 为 None（**不是 panic、不是残留样本**）。
    let empty = ValueStream::with_capacity(2);
    set.add("C08-负载-空值流无最新样本", empty.is_empty() && empty.latest().is_none(), "");

    // 热力三冗余：全档权重各自自洽。
    let weights = vec![0.0f32, 0.1, 0.24, 0.25, 0.5, 0.74, 0.75, 1.0];
    let mut b4 = bag();
    let cells = make_weight_heatmap(&weights, &mut b4);
    let mut all_ok = cells.len() == weights.len();
    let mut j = 0usize;
    while j < cells.len() {
        let mut probe = DiagBag::new();
        if !cells[j].redundant_consistent(&mut probe) {
            all_ok = false;
        }
        j += 1;
    }
    set.add("C08-负载-热力三冗余全档自洽", all_ok, "");
    // 标签是权重的百分数（独立重算，不问被测）。
    set.add("C08-负载-热力标签为权重的百分数", cells[4].label_pct == 50, "");
    // 权重钳制：越界两端都夹到 [0,1]。
    let mut b5 = bag();
    let lo = WeightCell::make(0, -3.0, &mut b5);
    let hi = WeightCell::make(1, 7.5, &mut b5);
    set.add("C08-负载-热力权重越界双端钳制", lo.weight == 0.0 && hi.weight == 1.0, "");
    set.add("C08-负载-热力越界有诊断", b5.has(DiagCode::WEIGHT_OUT_OF_RANGE), "");
    // NaN 权重 → 归零（否则三冗余全部失去意义）。
    let mut b6 = bag();
    let nanw = WeightCell::make(0, f32::NAN, &mut b6);
    set.add("C08-负载-热力NaN权重归零", nanw.weight == 0.0 && nanw.label_pct == 0, "");
}

/// 一个值流样本。
fn sample(t_ms: u32, v: f32) -> ValueSample {
    ValueSample { t_ms, track: 0, values: [v, 0.0, 0.0, 0.0], lanes: 1 }
}

// ---------------------------------------------------------------------------
// 二、按需拾取（洪水防线）
// ---------------------------------------------------------------------------

fn check_pick(set: &mut CheckSet) {
    let track = sc(t8(), s8());

    // **端点钉死**（本条最容易写错的地方）：抽稀后首尾必须是真实端点。
    let mut b = bag();
    let p = pick_curve(&track, 0, 100, 600, 3, Interp::Linear, &mut b);
    let endpoints_ok = p.points.len() == 3
        && p.points[0].t_ms == 100
        && p.points[p.points.len() - 1].t_ms == 600;
    set.add("C08-拾取-抽稀端点钉死（跨度不虚）", endpoints_ok, "");
    set.add("C08-拾取-抽稀标记与预算相符", p.decimated && p.delivered() == 3, "");
    set.add("C08-拾取-抽稀前真实帧数如实记账", p.total_keys == 6, "");
    set.add("C08-拾取-抽稀掉帧数可对账", p.dropped() == 3, "");
    set.add("C08-拾取-抽稀有诊断", b.has(DiagCode::PICK_BUDGET_EXHAUSTED), "");
    // 时刻严格递增（抽稀不得产出重复时刻——否则画线算法拿到零长线段）。
    let mut inc = true;
    let mut i = 1usize;
    while i < p.points.len() {
        if p.points[i].t_ms <= p.points[i - 1].t_ms {
            inc = false;
        }
        i += 1;
    }
    set.add("C08-拾取-抽稀后时刻严格递增", inc, "");
    // **工作单元与交付量无关**（抽稀也要看完每一帧）——这是「实测」的关键形态。
    set.add("C08-拾取-考察帧数不因抽稀而减少", p.scanned == 6 && p.scanned != p.delivered() as u32, "");

    // 预算恰好等于帧数：**不得抽稀**（边界不多抽一帧）。
    let mut b2 = bag();
    let p2 = pick_curve(&track, 0, 100, 600, 6, Interp::Linear, &mut b2);
    set.add("C08-拾取-预算等于帧数时不抽稀", !p2.decimated && p2.delivered() == 6, "");

    // 预算为 0 → 空 + 诊断，**绝不静默给全量**。
    let mut b3 = bag();
    let p3 = pick_curve(&track, 0, 0, 700, 0, Interp::Linear, &mut b3);
    set.add("C08-拾取-零预算返空且不静默", p3.points.is_empty() && b3.has(DiagCode::PICK_BUDGET_EXHAUSTED), "");

    // 预算 1 → 取窗首一点（绘制端至少有个锚）。
    let mut b4 = bag();
    let p4 = pick_curve(&track, 0, 0, 700, 1, Interp::Linear, &mut b4);
    set.add("C08-拾取-预算为1取窗首锚点", p4.points.len() == 1 && p4.points[0].t_ms == 0, "");

    // 窗边界：左闭右开语义。
    let mut b5 = bag();
    let p5 = pick_curve(&track, 0, 200, 400, 64, Interp::Linear, &mut b5);
    set.add("C08-拾取-窗为闭区间含两端", p5.total_keys == 3 && p5.points[0].t_ms == 200, "");

    // 窗口端点倒置：归一 + 显性。
    let mut b6 = bag();
    let p6 = pick_curve(&track, 0, 600, 100, 64, Interp::Linear, &mut b6);
    set.add("C08-拾取-窗口倒置归一且告警", p6.total_keys == 6 && b6.has(DiagCode::PICK_WINDOW_INVERTED), "");

    // 整窗落在时间轴**右端之外** → 钳制到末帧，且**必须显式记码**。
    // （早先这里静默返回端点，绘制端无法区分「钳制结果」与「真实窗内帧」。）
    let mut b7 = bag();
    let p7 = pick_curve(&track, 0, 800, 900, 64, Interp::Linear, &mut b7);
    set.add(
        "C08-拾取-轴外窗钳制到端点且显式记码",
        p7.points.len() == 1 && p7.points[0].t_ms == 700 && b7.has(DiagCode::PICK_WINDOW_CLAMPED),
        "",
    );
    // 单帧轴（时刻远离窗）→ 空 + 空窗诊断。
    let one = sc(vec![500], vec![1.0]);
    let mut b7c = bag();
    let p7d = pick_curve(&one, 0, 0, 100, 64, Interp::Linear, &mut b7c);
    set.add(
        "C08-拾取-单帧轴窗在轴左返空有诊断",
        p7d.points.is_empty() && b7c.has(DiagCode::PICK_WINDOW_EMPTY),
        "",
    );

    // 轨道下标越界：显性拒绝不 panic。
    let tracks = vec![sc(t8(), s8())];
    let mut b8 = bag();
    let p8 = pick_curve_by_index(&tracks, 9, 0, 700, 64, &mut b8);
    set.add("C08-拾取-轨道越界显性拒", p8.points.is_empty() && b8.has(DiagCode::PICK_TRACK_OUT_OF_RANGE), "");

    // **洪水防线：NaN 不进绘制缓冲**（曲线整屏消失的经典成因）。
    let nan_track = sc(vec![0, 100, 200], vec![0.0, f32::NAN, 2.0]);
    let mut b9 = bag();
    let p9 = pick_curve(&nan_track, 0, 0, 200, 64, Interp::Linear, &mut b9);
    let mut finite = true;
    let mut i2 = 0usize;
    while i2 < p9.points.len() {
        if !p9.points[i2].values[0].is_finite() {
            finite = false;
        }
        i2 += 1;
    }
    set.add("C08-拾取-NaN不进绘制缓冲", finite && p9.clamped == 1, "");
    set.add("C08-拾取-非有限值有告警", b9.has(DiagCode::CURVE_KEY_NON_FINITE), "");
    // Inf 同样钳制。
    let inf_track = sc(vec![0, 100], vec![f32::INFINITY, 1.0]);
    let mut b10 = bag();
    let p10 = pick_curve(&inf_track, 0, 0, 100, 64, Interp::Linear, &mut b10);
    set.add("C08-拾取-Inf同样钳制", p10.points[0].values[0].is_finite() && p10.clamped == 1, "");

    // 空轨：空拾取 + 诊断。
    let empty_track = sc(vec![], vec![]);
    let mut b11 = bag();
    let p11 = pick_curve(&empty_track, 0, 0, 700, 64, Interp::Linear, &mut b11);
    set.add("C08-拾取-空轨返空有诊断", p11.points.is_empty() && b11.has(DiagCode::PICK_WINDOW_EMPTY), "");

    // 大洪水：5000 帧万级抽稀仍受预算约束（**不因大输入失控**）。
    let mut times: Vec<u32> = Vec::new();
    let mut vals: Vec<f32> = Vec::new();
    let mut n = 0u32;
    while n < 5000 {
        times.push(n * 10);
        vals.push(n as f32);
        n += 1;
    }
    let big = sc(times, vals);
    let mut b12 = bag();
    let p12 = pick_curve(&big, 0, 0, 50_000, 64, Interp::Linear, &mut b12);
    set.add(
        "C08-拾取-万帧洪水仍守预算",
        p12.decimated && p12.delivered() == 64 && p12.total_keys == 5000,
        "",
    );
    set.add("C08-拾取-洪水下端点仍钉死", p12.points[0].t_ms == 0 && p12.points[63].t_ms == 49_990, "");
}

// ---------------------------------------------------------------------------
// 三、协议家族（F1946）
// ---------------------------------------------------------------------------

fn check_protocol(set: &mut CheckSet) {
    // 工作单元是实测：轨道越长，重取成本越高（**非自证式常数**）。
    let mut m_short = mirror_with_keys(4);
    let mut m_long = mirror_with_keys(120);
    let mut c1 = TuneChannel::default();
    let mut c2 = TuneChannel::default();
    let mut b = bag();
    let a1 = apply_tune(
        &mut c1,
        &mut m_short,
        TuneCommand { seq: 1, op: TuneOp::SetKeyValue { track: 0, key: 0, channel: 0, value: 9.0 } },
        &mut b,
    );
    let a2 = apply_tune(
        &mut c2,
        &mut m_long,
        TuneCommand { seq: 1, op: TuneOp::SetKeyValue { track: 0, key: 0, channel: 0, value: 9.0 } },
        &mut b,
    );
    let (w1, w2) = (work_of(a1), work_of(a2));
    set.add("C08-协议-工作单元随轨长实测增长", w2 > w1, "");
    set.add("C08-协议-短轨改值不超一帧预算", w1 <= TUNE_WORK_BUDGET, "");
    set.add("C08-协议-长轨改值如实判超预算", w2 > TUNE_WORK_BUDGET, "");
    // 超预算只在长轨那条上发生（短轨那条**不得**被连带判超——
    // 那样这条判据就退化成「恒为真的总数检查」）。
    set.add("C08-协议-超预算按轨长分别记账", c2.over_budget == 1 && c1.over_budget == 0, "");
    set.add("C08-协议-超预算有P1立案", b.has(DiagCode::TUNE_WORK_OVER_BUDGET), "");
    // 改值真的落到轨上（**不是只回 ACK**）。
    set.add("C08-协议-改值真落到轨道", m_short.tracks[0].channels[0] == 9.0, "");
    // 改值触发修订号递增（F2407 缓存按此判脏——跨单源联动）。
    set.add("C08-协议-改值递增修订号", m_short.tracks[0].edit_rev == 1, "");
    // 曲线标脏（绘制端要重取）。
    set.add("C08-协议-改值标脏曲线", m_short.curve_dirty[0], "");
    set.add("C08-协议-累计工作单元可对账", c2.total_work == w2 as u64, "");
    set.add("C08-协议-交付量与通道一致", m_long.total_delivered == w2, "");

    // **序号水位线两语义**（本条最易写错的判别点）。
    let mut m3 = mirror_with_keys(4);
    let mut c3 = TuneChannel::default();
    let mut b2 = bag();
    let _ = apply_tune(
        &mut c3,
        &mut m3,
        TuneCommand { seq: 5, op: TuneOp::SetWeight { track: 0, weight: 0.5 } },
        &mut b2,
    );
    set.add("C08-协议-生效后水位线推进", c3.last_seq == 5, "");
    // 重放（seq 回退）→ 拒，且**水位线不动**。
    //
    // **必须用严格更小的 seq 重放**（此处 3 < 已生效的 5）。用**同一个**
    // seq 重放是弱门禁：变异体「重放时把 last_seq 写成 cmd.seq」在此情形下
    // 写入的就是原值，判据前后都读到 5，**恒绿**。变异验证 M07 实测漏网，
    // 根因正是此处——水位线必须被**实测到变化**，才谈得上「不动」。
    let before = c3.last_seq;
    let mut b3 = bag();
    let r1 = apply_tune(
        &mut c3,
        &mut m3,
        TuneCommand { seq: 3, op: TuneOp::SetWeight { track: 0, weight: 0.9 } },
        &mut b3,
    );
    set.add("C08-协议-重放被拒且水位线不动", !r1.is_applied() && r1.is_seq_reject() && c3.last_seq == before, "");
    // **水位线未被回退拉低**：重放 seq=3 之后水位线仍须是 5（若实现写成
    // `last_seq = cmd.seq`，这里会读到 3 —— 与上一条互补，堵死同一个漏网口）。
    set.add("C08-协议-重放不回退水位线", c3.last_seq == 5, "");
    set.add("C08-协议-重放有告警", b3.has(DiagCode::TUNE_SEQ_REGRESSED), "");
    // 重放后权重**未被改**（真拒，非「拒了但改了」）。
    set.add("C08-协议-重放未改实际状态", m3.weights[0] == 0.5, "");

    // 参数非法（下标越界）而拒 → **水位线必须推进**（否则无限重投）。
    let mut m4 = mirror_with_keys(4);
    let mut c4 = TuneChannel::default();
    let mut b4 = bag();
    let r2 = apply_tune(
        &mut c4,
        &mut m4,
        TuneCommand { seq: 3, op: TuneOp::SetKeyValue { track: 0, key: 99, channel: 0, value: 1.0 } },
        &mut b4,
    );
    set.add("C08-协议-参数拒推进水位线", !r2.is_applied() && c4.last_seq == 3, "");
    set.add("C08-协议-参数拒与序号拒可区分", !r2.is_seq_reject() && r1.is_seq_reject(), "");

    // **时刻改写守卫**：破坏单调必须拒（否则二分前提被毁且静默）。
    let mut m5 = mirror_with_keys(4);
    let mut c5 = TuneChannel::default();
    let mut b5 = bag();
    let bad = apply_tune(
        &mut c5,
        &mut m5,
        TuneCommand { seq: 1, op: TuneOp::SetKeyTime { track: 0, key: 2, t_ms: 5 } },
        &mut b5,
    );
    set.add("C08-协议-破坏单调的时刻改写被拒", !bad.is_applied(), "");
    set.add("C08-协议-破坏单调有专属码", b5.has(DiagCode::TIMES_NOT_MONOTONIC), "");
    set.add("C08-协议-被拒后时间轴未变", m5.tracks[0].times[2] == 200, "");
    // 合法时刻改写（落在前后邻居之间）→ 生效。
    let mut m6 = mirror_with_keys(4);
    let mut c6 = TuneChannel::default();
    let mut b6 = bag();
    let good = apply_tune(
        &mut c6,
        &mut m6,
        TuneCommand { seq: 1, op: TuneOp::SetKeyTime { track: 0, key: 2, t_ms: 250 } },
        &mut b6,
    );
    set.add("C08-协议-合法时刻改写生效", good.is_applied() && m6.tracks[0].times[2] == 250, "");

    // 权重指令越界。
    let mut m7 = mirror_with_keys(2);
    let mut c7 = TuneChannel::default();
    let mut b7 = bag();
    let r3 = apply_tune(
        &mut c7,
        &mut m7,
        TuneCommand { seq: 1, op: TuneOp::SetWeight { track: 77, weight: 0.5 } },
        &mut b7,
    );
    set.add("C08-协议-权重下标越界被拒", !r3.is_applied() && b7.has(DiagCode::PICK_TRACK_OUT_OF_RANGE), "");
    // 权重越界值被钳制而非拒（**钳制优于丢弃**：编辑器拖出 [0,1] 是常态）。
    let mut m8 = mirror_with_keys(2);
    let mut c8 = TuneChannel::default();
    let mut b8 = bag();
    let r4 = apply_tune(
        &mut c8,
        &mut m8,
        TuneCommand { seq: 1, op: TuneOp::SetWeight { track: 0, weight: 5.0 } },
        &mut b8,
    );
    set.add("C08-协议-权重越界值被钳制", r4.is_applied() && m8.weights[0] == 1.0, "");
    // 通道号越界 → 拒（**不越界写**）。
    let mut m9 = mirror_with_keys(4);
    let mut c9 = TuneChannel::default();
    let mut b9 = bag();
    let r5 = apply_tune(
        &mut c9,
        &mut m9,
        TuneCommand { seq: 1, op: TuneOp::SetKeyValue { track: 0, key: 0, channel: 9, value: 1.0 } },
        &mut b9,
    );
    set.add("C08-协议-通道号越界被拒不越界写", !r5.is_applied() && m9.tracks[0].channels[0] == 0.0, "");
    // ACK 序号与指令一致。
    set.add("C08-协议-ACK序号回传正确", good.seq() == 1, "");

    // **轨道下标与帧下标是两个独立下标**（变异回归：早先把两者合成一个，
    // 于是「改第 2 轨第 3 帧」会静默落到别的目标上）。此处用**两条轨**
    // 构造：只改第 1 轨（track=1）的第 2 帧，第 0 轨必须**分毫未动**。
    let mut two = TrackMirror {
        tracks: vec![
            sc(vec![0, 100, 200, 300], vec![0.0, 1.0, 2.0, 3.0]),
            sc(vec![0, 100, 200, 300], vec![10.0, 11.0, 12.0, 13.0]),
        ],
        weights: vec![0.0, 0.0],
        curve_dirty: vec![false, false],
        total_delivered: 0,
    };
    let mut c10 = TuneChannel::default();
    let mut b10a = bag();
    let r10 = apply_tune(
        &mut c10,
        &mut two,
        TuneCommand { seq: 1, op: TuneOp::SetKeyValue { track: 1, key: 2, channel: 0, value: 99.0 } },
        &mut b10a,
    );
    set.add(
        "C08-协议-轨下标与帧下标独立寻址",
        r10.is_applied()
            && two.tracks[1].channels[2] == 99.0
            && two.tracks[0].channels[2] == 2.0,
        "",
    );
    set.add("C08-协议-只标脏被改的那一轨", two.curve_dirty[1] && !two.curve_dirty[0], "");
    // 帧下标越界（轨存在但帧不存在）→ 拒，**不越界写**。
    let mut c11 = TuneChannel::default();
    let mut b10b = bag();
    let r11 = apply_tune(
        &mut c11,
        &mut two,
        TuneCommand { seq: 2, op: TuneOp::SetKeyValue { track: 1, key: 88, channel: 0, value: 1.0 } },
        &mut b10b,
    );
    set.add("C08-协议-帧下标越界被拒不越界写", !r11.is_applied() && two.tracks[1].channels.len() == 4, "");
    // 轨道下标越界 → 拒。
    let mut c12 = TuneChannel::default();
    let mut b10c = bag();
    let r12 = apply_tune(
        &mut c12,
        &mut two,
        TuneCommand { seq: 3, op: TuneOp::SetKeyTime { track: 77, key: 0, t_ms: 10 } },
        &mut b10c,
    );
    set.add("C08-协议-轨下标越界被拒", !r12.is_applied(), "");
    // 统计流：占比口径 + 分母为零返未定义。
    let mut w = StatWindow::new();
    let mut b10 = bag();
    let _ = w.observe(StatSample { active_tracks: 9, eval_us: 250, frame_us: 1000 }, &tele(3, 1), &mut b10);
    let s = w.summary(&mut b10);
    set.add("C08-协议-耗时占比口径为对帧耗时", s.eval_share_ppm == Some(250_000), "");
    set.add("C08-协议-缓存命中率口径为命中除以总量", s.cache_hit_ppm == Some(750_000), "");
    set.add("C08-协议-活跃轨道数如实", s.active_tracks == 9 && s.peak_active_tracks == 9, "");
    // 分母为零 → **未定义而非 0**。
    let mut w0 = StatWindow::new();
    let mut b11 = bag();
    let _ = w0.observe(StatSample { active_tracks: 1, eval_us: 0, frame_us: 0 }, &tele(0, 0), &mut b11);
    let s0 = w0.summary(&mut b11);
    set.add("C08-协议-零分母返未定义而非0", s0.eval_share_ppm.is_none() && s0.cache_hit_ppm.is_none(), "");
    set.add("C08-协议-零分母有告警", b11.has(DiagCode::STATS_RATIO_UNDEFINED), "");
    // 超帧耗时如实标记。
    let mut w2 = StatWindow::new();
    let mut b12 = bag();
    let _ = w2.observe(StatSample { active_tracks: 1, eval_us: 900, frame_us: 100 }, &tele(1, 1), &mut b12);
    set.add("C08-协议-超帧耗时如实标记", w2.summary(&mut b12).eval_over_budget, "");
    // 统计洪水 → 降档（样本折半、步长翻倍、**不混档**）。
    let mut w3 = StatWindow::new();
    let mut b13 = bag();
    let mut down = false;
    let mut f = 0u32;
    while f < STAT_WINDOW_MAX_SAMPLES {
        if w3.observe(StatSample { active_tracks: 2, eval_us: 10, frame_us: 100 }, &tele(1, 1), &mut b13) {
            down = true;
        }
        f += 1;
    }
    set.add("C08-协议-统计洪水触发降档", down && w3.downgrades() == 1, "");
    set.add("C08-协议-降档后步长翻倍", w3.stride() == 2, "");
    set.add("C08-协议-降档后样本折半", w3.samples() == STAT_WINDOW_MAX_SAMPLES / 2, "");
    set.add("C08-协议-降档有诊断", b13.has(DiagCode::STATS_DOWNGRADE), "");
    // **未触顶时绝不降档**（降档不是无条件行为）。
    let mut w4 = StatWindow::new();
    let mut b14 = bag();
    let mut no_down = true;
    let mut f2 = 0u32;
    while f2 < 10 {
        if w4.observe(StatSample { active_tracks: 1, eval_us: 1, frame_us: 10 }, &tele(1, 1), &mut b14) {
            no_down = false;
        }
        f2 += 1;
    }
    set.add("C08-协议-未触顶不降档", no_down && w4.stride() == 1, "");
}

/// 造一条含 `keys` 关键帧的镜像。
fn mirror_with_keys(keys: usize) -> TrackMirror {
    let mut times: Vec<u32> = Vec::new();
    let mut chans: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i < keys {
        times.push(i as u32 * 100);
        chans.push(i as f32);
        i += 1;
    }
    let mut dirty: Vec<bool> = Vec::new();
    let mut w: Vec<f32> = Vec::new();
    i = 0;
    while i < keys {
        dirty.push(false);
        w.push(0.0);
        i += 1;
    }
    TrackMirror { tracks: vec![sc(times, chans)], weights: w, curve_dirty: dirty, total_delivered: 0 }
}

/// 取 ACK 的工作单元（非 Applied 则 0）。
fn work_of(a: TuneAck) -> u32 {
    match a {
        TuneAck::Applied { work_units, .. } => work_units,
        TuneAck::Rejected { .. } => 0,
    }
}

// ---------------------------------------------------------------------------
// 四、M 段信封
// ---------------------------------------------------------------------------

fn check_envelope(set: &mut CheckSet) {
    // 四类型注册齐备。
    let mut reg = EnvelopeRegistry::new();
    let mut i = 0usize;
    while i < PayloadKind::ALL.len() {
        let _ = reg.register(Envelope::new(PayloadKind::ALL[i], 128 * (i as u32 + 1), i as u32));
        i += 1;
    }
    set.add("C08-信封-四类型齐备", reg.complete() && reg.registered() == M_SEGMENT_SLOTS, "");
    set.add("C08-信封-注册代数如实累加", reg.epoch() == M_SEGMENT_SLOTS as u32, "");
    set.add("C08-信封-未拦截时消费端可取", reg.take(PayloadKind::Curve).is_some(), "");
    set.add("C08-信封-线上编码互异", wires_unique(), "");
    set.add("C08-信封-编码反查可往返", PayloadKind::from_wire(0x03) == Some(PayloadKind::Weight), "");
    set.add("C08-信封-非法编码反查为None", PayloadKind::from_wire(0xFF).is_none(), "");
    set.add("C08-信封-家族声明一致", family_is_consistent(), "");
    // 自摘要与重算一致（**对账基准本身要可信**）。
    let e = Envelope::new(PayloadKind::Stats, 512, 7);
    set.add("C08-信封-自摘要与重算一致", !e.drifted() && e.checksum() == e.declared, "");
    // **摘要对 seq 敏感**：换 seq 必须改摘要（否则「对账」是摆设）。
    let e2 = Envelope::new(PayloadKind::Stats, 512, 8);
    set.add("C08-信封-摘要对序号敏感", e.checksum() != e2.checksum(), "");
    // 摘要对字节数敏感。
    let e3 = Envelope::new(PayloadKind::Stats, 513, 7);
    set.add("C08-信封-摘要对载荷长度敏感", e.checksum() != e3.checksum(), "");
    // 摘要对类型敏感。
    let e4 = Envelope::new(PayloadKind::Weight, 512, 7);
    set.add("C08-信封-摘要对负载类型敏感", e.checksum() != e4.checksum(), "");

    // 干净表对账 → 0 漂移、**不拦截**。
    let mut reg2 = EnvelopeRegistry::new();
    let mut i2 = 0usize;
    while i2 < PayloadKind::ALL.len() {
        let _ = reg2.register(Envelope::new(PayloadKind::ALL[i2], 256, i2 as u32));
        i2 += 1;
    }
    let mut b = bag();
    let d = reg2.reconcile(&mut b);
    set.add("C08-信封-干净表零漂移", d == 0 && !reg2.intercepted(), "");
    set.add("C08-信封-干净表无漂移告警", !b.has(DiagCode::ENVELOPE_DRIFT), "");

    // **漂移必须拦截**（本条核心：只记一笔不拦截 = 红线降级成日志）。
    let mut reg3 = EnvelopeRegistry::new();
    let mut env = Envelope::new(PayloadKind::Curve, 1024, 1);
    env.byte_len = 9999; // 模拟载荷被改写而声明未更
    let _ = reg3.register(env);
    let mut b2 = bag();
    let d2 = reg3.reconcile(&mut b2);
    set.add("C08-信封-漂移被检出", d2 == 1, "");
    set.add("C08-信封-漂移即置拦截", reg3.intercepted(), "");
    set.add("C08-信封-漂移有P1立案", b2.p1_count() == 1, "");
    set.add("C08-信封-拦截后消费端一律取不到", reg3.take(PayloadKind::Curve).is_none(), "");
    // **peek 仍可见**（对账自身要能看到漂移件，否则无从修）。
    set.add("C08-信封-拦截后对账仍可窥见", reg3.peek(PayloadKind::Curve).is_some(), "");
    set.add("C08-信封-拦截后其它类型也取不到", reg3.take(PayloadKind::Stats).is_none(), "");
    set.add("C08-信封-漂移次数累加", reg3.drift_count() == 1, "");
    // 二次对账仍检出（幂等，不是只报一次）。
    let mut b3 = bag();
    let d3 = reg3.reconcile(&mut b3);
    set.add("C08-信封-重复对账仍检出", d3 == 1 && reg3.drift_count() == 2, "");
    // **解拦截后恢复取用**（拦截不是不可逆的永久封禁）。
    reg3.clear_intercept();
    set.add("C08-信封-解拦截后恢复取用", !reg3.intercepted() && reg3.take(PayloadKind::Curve).is_some(), "");

    // 未注册类型取不到。
    let mut reg4 = EnvelopeRegistry::new();
    let _ = reg4.register(Envelope::new(PayloadKind::Curve, 64, 0));
    set.add("C08-信封-未注册类型取不到", reg4.take(PayloadKind::Weight).is_none(), "");
    set.add("C08-信封-部分注册不判齐备", !reg4.complete(), "");
}

// ---------------------------------------------------------------------------
// 五、零静默
// ---------------------------------------------------------------------------

fn check_explicit(set: &mut CheckSet) {
    set.add("C08-显性-诊断码齐备17项", DiagCode::ALL.len() == 17, "");
    set.add("C08-显性-码标签互异", labels_unique(), "");
    // 码值互异（防两码同值——那会让 count() 无法区分）。
    let mut uniq = true;
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            if DiagCode::ALL[i].0 == DiagCode::ALL[j].0 {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add("C08-显性-码值互异", uniq, "");
    // 码段不与 F2407（0x2Axx）/F2406（6001 段）重叠。
    let mut no_clash = true;
    let mut i2 = 0usize;
    while i2 < DiagCode::ALL.len() {
        // 本条独占高字节 0x2B：既不与 F2407 的 0x2Axx 撞段，
        // 也不与 F2406 的 6001 段（高字节 0x17）撞段。
        if (DiagCode::ALL[i2].0 >> 8) != 0x2B {
            no_clash = false;
        }
        i2 += 1;
    }
    set.add("C08-显性-码段不与前序条目重叠", no_clash, "");
    // 诊断渲染非空（诊断面不是哑巴）。
    let mut b = bag();
    b.push(DiagCode::ENVELOPE_DRIFT);
    b.push_p1(DiagCode::STRIP_FAILED);
    set.add("C08-显性-诊断渲染非空", !b.render().is_empty(), "");
    set.add("C08-显性-按码计数准确", b.count(DiagCode::ENVELOPE_DRIFT) == 1, "");
    set.add("C08-显性-按严重度计数准确", b.count_severity(Severity::P1) == 1, "");
    set.add("C08-显性-查有无准确", b.has(DiagCode::STRIP_FAILED) && !b.has(DiagCode::PICK_WINDOW_EMPTY), "");
    set.add("C08-显性-空袋真的空", bag().is_empty(), "");
    set.add("C08-显性-未知码有人话兜底", DiagCode(0x7777).label() == "未登记诊断码", "");
    set.add("C08-显性-负载类型标签齐备", PayloadKind::Curve.label() == "曲线可视", "");
}

// ---------------------------------------------------------------------------
// 六、剔除零成本（双向验证）
// ---------------------------------------------------------------------------

fn check_strip(set: &mut CheckSet) {
    // **对照组**：Debug 档必须**真的递增**——否则「Release 为 0」是恒真弱门禁。
    let mut d = StripGuard::new(BuildProfile::Debug);
    let mut b = bag();
    let ok1 = d.try_build(&mut b);
    let ok2 = d.try_build(&mut b);
    set.add("C08-剔除-Debug档对照组真的递增", ok1 && ok2 && d.payload_builds == 2, "");
    set.add("C08-剔除-Debug档无剔除告警", !b.has(DiagCode::STRIP_FAILED), "");

    // Release 档：正常请求与**强行尝试**都不递增。
    let mut r = StripGuard::new(BuildProfile::Release);
    let mut b2 = bag();
    let ok3 = r.try_build(&mut b2);
    let ok4 = r.try_build(&mut b2);
    set.add("C08-剔除-Release档构建计数恒零含强行路径", r.payload_builds == 0, "");
    set.add("C08-剔除-Release档请求被拒", !ok3 && !ok4, "");
    set.add("C08-剔除-Release档强行次数如实记账", r.forced_attempts == 2, "");
    set.add("C08-剔除-Release档剔除失败有P1", b2.p1_count() == 2, "");
    set.add("C08-剔除-档位判定与构造一致", !r.payloads_enabled() && d.payloads_enabled(), "");
    // 默认档是 Debug（**安全默认**：忘配的发行构建不会静默带负载）。
    let def = StripGuard::default();
    set.add("C08-剔除-默认档为开发形态", def.payloads_enabled(), "");
    // 1000 次强行请求仍恒零（**不是「少数几次恰好为 0」**）。
    let mut r2 = StripGuard::new(BuildProfile::Release);
    let mut b3 = bag();
    let mut n = 0u32;
    while n < 1000 {
        let _ = r2.try_build(&mut b3);
        n += 1;
    }
    set.add("C08-剔除-千次强行请求后仍恒零", r2.payload_builds == 0 && r2.forced_attempts == 1000, "");
}

// ---------------------------------------------------------------------------
// 七、无障碍（三冗余的可分性 —— 弱门禁自律的关键）
// ---------------------------------------------------------------------------

fn check_a11y(set: &mut CheckSet) {
    // **判据侧自己举反例**：找出一对「同色不同尺寸」的权重。
    // 若找不到，说明两套分档边界退化成同一份数据，三冗余是假的。
    let mut b = bag();
    let mut witness: Option<(WeightCell, WeightCell)> = None;
    let probe = [0.0f32, 0.05, 0.10, 0.15, 0.20, 0.3, 0.4, 0.6, 0.8, 0.9];
    let mut i = 0usize;
    while i < probe.len() {
        let mut j = i + 1;
        while j < probe.len() {
            let a = WeightCell::make(0, probe[i], &mut b);
            let c = WeightCell::make(1, probe[j], &mut b);
            if a.channels_independent(&c) {
                witness = Some((a, c));
                j = probe.len();
            } else {
                j += 1;
            }
        }
        i += 1;
    }
    let has_witness = match witness {
        Some((a, c)) => a.color == c.color && a.size != c.size,
        None => false,
    };
    set.add("C08-无障碍-存在同色不同尺寸的反例对", has_witness, "");
    // 色档四档全覆盖（**满位形态**，不是只测两档）。
    set.add(
        "C08-无障碍-色档四档全覆盖",
        color_band(0.1) == HeatColor::Cool
            && color_band(0.3) == HeatColor::Warm
            && color_band(0.6) == HeatColor::Hot
            && color_band(0.9) == HeatColor::Critical,
        "",
    );
    set.add(
        "C08-无障碍-尺寸档四档全覆盖",
        size_band(0.05) == HeatSize::Small
            && size_band(0.2) == HeatSize::Medium
            && size_band(0.5) == HeatSize::Large
            && size_band(0.9) == HeatSize::XLarge,
        "",
    );
    // 数值标签独立于两档（0..100 连续取值，**不与色档一一对应**）。
    let mut b2 = bag();
    let c1 = WeightCell::make(0, 0.30, &mut b2);
    let c2 = WeightCell::make(0, 0.35, &mut b2);
    set.add(
        "C08-无障碍-标签分辨率高于色档",
        c1.color == c2.color && c1.label_pct != c2.label_pct,
        "",
    );
    // **冗余漂移可检出**：把 color 改坏，自洽判据必须转红并立案。
    let mut broken = WeightCell::make(0, 0.8, &mut b2);
    broken.color = HeatColor::Cool;
    let mut b3 = bag();
    let ok = broken.redundant_consistent(&mut b3);
    set.add("C08-无障碍-冗余漂移可检出并立案", !ok && b3.has(DiagCode::REDUNDANCY_DRIFT), "");
    // 标签四舍五入口径（0.5 → 1%）。
    set.add("C08-无障碍-标签四舍五入口径正确", label_of(0.005) == 1 && label_of(0.0) == 0, "");
    set.add("C08-无障碍-标签上下界夹紧", label_of(-1.0) == 0 && label_of(2.0) == 100, "");
    // 描述与冒烟非空（**交付面不是哑巴**）。
    set.add("C08-无障碍-描述非空", !describe().is_empty(), "");
    set.add("C08-无障碍-冒烟非空", !smoke().is_empty(), "");
}