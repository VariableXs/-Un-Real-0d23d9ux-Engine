//! VE-F0214 域自检（判据逐条映射锚点，26 项）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0214`
//!
//! 判据设计纪律（本单实测得出，VE-F0213 补判据时踩过）：
//! 1. **不得用表内元素验表内函数**——查表/分位这类必须用表外真实形态。
//! 2. **阈值不得同时充当预期值**——阈值来自规格常量时，要另用**锚点字面量**
//!    断言一次，并另断言常量等于该字面量，否则「把阈值改成 astronomically 大」
//!    仍然全绿。
//! 3. **缺测不能记 0**——判据要能区分「缺测」与「真实的零」。
//! 4. **饱和要可区分**——恰好等于饱和线 ≠ 已饱和，判据必须能分辨二者。

use crate::checks::CheckSet;
use crate::svstar2::veb14_perf::*;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格锚点字面量（**不引用实现常量**，否则改常量=改预期，自证）
// ---------------------------------------------------------------------------

/// 锚点「分位草图」——草图桶数锚点为 16。
const ANCHOR_SKETCH_BUCKETS: usize = 16;
/// 锚点「按需快照不常驻」——队列槽数规格未给，取 8 作有界性锚点。
const ANCHOR_QUEUE_SLOTS: usize = 8;
/// 锚点 F0096「降采样阶梯 100%/50%/10%」。
const ANCHOR_STEPS: [u8; 3] = [100, 50, 10];
/// 锚点「计数溢出 → 饱和不回绕」——饱和线取 u32::MAX（u64 累加器的可表示上界之下）。
const ANCHOR_SAT: u64 = 0xFFFF_FFFF;

/// 独立算样本值所属桶区间 `[lo, hi)`（对数分桶：桶 i 覆盖 `[1<<i, 1<<(i+1))`）。
///
/// **不复用实现里的分桶逻辑**——判据自己按同一口径重算一遍，实现若把
/// 分桶写错（如用 `v as u8` 取低位），这里与实现就会落在不同桶从而转红。
/// 若两者同源则改实现=改判据，恒真弱门禁。
fn bucket_span(v: u64) -> (u64, u64) {
    if v == 0 {
        return (0, 1);
    }
    let i = 63 - v.leading_zeros();
    (1u64 << i, 1u64 << (i + 1))
}

/// 打一帧：n 条命令、n*字节、有延迟。
///
/// **注意**：`mark_complete()` 内部已 `latency.record()`，此处**不得**再
/// 直接 `latency.record()`——否则一帧记两个样本，`latency_samples`
/// 断言与分位分布全部失真（承 F0213 教训：写判据前先读真实 API）。
fn frame(id: u64, cmds: u64, bytes: u64, lat: u64, measured: bool) -> FrameCounters {
    let mut f = FrameCounters::new(id);
    f.add_commands(cmds);
    f.add_bytes(bytes);
    f.observe_queue_depth(4);
    if measured {
        f.mark_submit();
        f.mark_complete(lat);
    } else {
        f.mark_missing();
    }
    f
}

// ===========================================================================
// 一、饱和计数（锚点「计数溢出→饱和不回绕」）
// ===========================================================================

fn c214_saturated() -> Vec<(&'static str, bool)> {
    let mut c = SaturatedCounter::new();
    let mut v = Vec::new();
    // 正常累加
    c.add(10);
    v.push(("C214-饱和-正常累加", c.value() == 10 && !c.is_saturated()));
    // 恰好等于饱和线：值对、但**不算饱和**（可区分「恰好」与「溢出」）
    c.reset();
    c.add_saturating(ANCHOR_SAT);
    v.push((
        "C214-饱和-恰好等于上限不算饱和",
        c.value() == ANCHOR_SAT && !c.is_saturated(),
    ));
    // 再加 1 → 溢出：钉在上限 + 置饱和位（**不回绕**）
    c.add(1);
    v.push((
        "C214-饱和-溢出后钉住不回绕",
        c.value() == ANCHOR_SAT && c.is_saturated(),
    ));
    // 饱和后再累加：值不得变（否则又变成「涨回去了」）
    let before = c.value();
    c.add(999);
    v.push(("C214-饱和-饱和后累加不改值", c.value() == before));
    // 常量对齐：饱和线必须等于锚点字面量
    v.push((
        "C214-饱和-饱和线对齐锚点",
        COUNTER_SAT == ANCHOR_SAT,
    ));
    v
}

// ===========================================================================
// 二、分位草图（锚点「分位在线更新 O(1)」）
// ===========================================================================

fn c214_sketch() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 桶数对齐锚点
    v.push((
        "C214-分位-桶数对齐锚点",
        SKETCH_BUCKETS == ANCHOR_SKETCH_BUCKETS,
    ));
    // 空草图：无样本
    let empty = QuantileSketch::new();
    v.push(("C214-分位-空草图无样本", empty.count() == 0));
    // 常量 vs 桶数自洽
    v.push((
        "C214-分位-最大位移与桶数自洽",
        SKETCH_MAX_SHIFT as usize == SKETCH_BUCKETS - 1,
    ));

    // 单点：分位必须落回该点所在桶区间（不是恒返回常数）。
    // 区间由**样本值本身**独立算出（`bucket_span`），不引用实现常量，
    // 避免「用表内元素验表内函数」。
    let mut s = QuantileSketch::new();
    s.record(1000);
    let q = s.quantile(50);
    let (lo1, hi1) = bucket_span(1000);
    v.push((
        "C214-分位-单点分位落本桶",
        s.count() == 1 && q >= lo1 && q < hi1,
    ));
    // 越界读桶返回 0 而非 panic
    v.push((
        "C214-分位-越界读桶返0",
        s.bucket(SKETCH_BUCKETS) == 0 && s.bucket(9999) == 0,
    ));

    // **单调性 + 分布形状**：1000 个样本，90% 在 100µs、10% 在 1000µs
    // 判别：p50 应在 100µs 桶、p99 应在 1000µs 桶。
    // 这是关键判据——若分位写死返回某常数，p50 与 p99 会相同，判据转红。
    let mut m = QuantileSketch::new();
    let mut i = 0;
    while i < 900 {
        m.record(100);
        i += 1;
    }
    i = 0;
    while i < 100 {
        m.record(1000);
        i += 1;
    }
    let p50 = m.quantile(50);
    let p90 = m.quantile(90);
    let p99 = m.quantile(99);
    let (lo100, hi100) = bucket_span(100);
    let (lo1000, hi1000) = bucket_span(1000);
    v.push(("C214-分位-样本数对账", m.count() == 1000));
    v.push((
        "C214-分位-p50落低桶",
        p50 >= lo100 && p50 < hi100,
    ));
    v.push((
        "C214-分位-p99落高桶",
        p99 >= lo1000 && p99 < hi1000,
    ));
    v.push((
        "C214-分位-分位单调p50<=p90<=p99",
        p50 <= p90 && p90 <= p99,
    ));
    v.push((
        "C214-分位-三个分位不全是同一值",
        !(p50 == p90 && p90 == p99),
    ));
    // 非空桶 ≥ 2（两个不同量级 ⇒ 至少两个桶）
    v.push((
        "C214-分位-占用桶数反映双量级",
        m.occupied_buckets() >= 2,
    ));

    // 桶计数总和 == 样本数（分桶不丢样本、不重复计）
    let mut sum = 0u64;
    let mut k = 0;
    while k < SKETCH_BUCKETS {
        sum += m.bucket(k) as u64;
        k += 1;
    }
    v.push(("C214-分位-桶计数总和等于样本数", sum == 1000));

    // q 参数越界收口到 100（不得 panic 也不得返 0）
    v.push((
        "C214-分位-q越界收口",
        m.quantile(200) == m.quantile(100) && m.quantile(100) > 0,
    ));
    // 0 样本不产生非零分位（避免「无数据当零延迟」）
    v.push((
        "C214-分位-空草图分位为0非缺测替代",
        empty.quantile(99) == 0 && empty.count() == 0,
    ));
    v
}

// ===========================================================================
// 三、帧计数与缺测（锚点「打点缺失→该帧指标标记缺测」）
// ===========================================================================

fn c214_frames() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 正常帧：三个计数与延迟都对
    let mut p = VirtioPerf::new();
    p.submit_frame(frame(1, 7, 4096, 2000, true));
    let s = p.snapshot();
    v.push(("C214-帧计-命令数对账", s.total_commands.0 == 7));
    v.push(("C214-帧计-字节数对账", s.total_bytes.0 == 4096));
    v.push(("C214-帧计-已测帧计数", s.frames_measured == 1 && s.frames_missing == 0));
    // 延迟分位：单样本 2000µs 落 `[2048, 4096)`？——不，2000 落在
    // `[1024, 2048)` 桶（2^10=1024 ≤ 2000 < 2^11=2048）。桶内插值只会
    // 给出该桶内的值，故判据按**桶区间**断言而非「≥ 原样本值」。
    let (lo2k, hi2k) = bucket_span(2000);
    v.push((
        "C214-帧计-延迟样本非零",
        s.latency_samples == 1 && s.p50_us >= lo2k && s.p50_us < hi2k,
    ));

    // 队列深度是**水位不是总量**：同一帧多次观测取最大
    let mut f = FrameCounters::new(2);
    f.observe_queue_depth(7);
    f.observe_queue_depth(3);
    f.observe_queue_depth(9);
    f.observe_queue_depth(2);
    v.push((
        "C214-帧计-队列深度取最大非累加",
        f.queue_depth.value() == 9,
    ));

    // 缺测帧：标记 missing 且**不产生延迟样本**（缺测 ≠ 零延迟）
    let mut p2 = VirtioPerf::new();
    p2.submit_frame(frame(1, 5, 1000, 0, true));
    p2.submit_frame(frame(2, 5, 1000, 0, false)); // 缺测
    let s2 = p2.snapshot();
    v.push((
        "C214-缺测-缺测帧被标记",
        s2.frames_missing == 1 && s2.frames_measured == 1,
    ));
    v.push((
        "C214-缺测-缺测不产生延迟样本",
        s2.latency_samples == 1,
    ));
    v.push((
        "C214-缺测-缺测占比可算",
        s2.missing_pct == 50,
    ));
    // **关键区分**：缺测帧不得把延迟拉向 0——若实现把缺测记 0 参与分位，
    // p50 会掉到 0 桶；正确实现下 p50 仍应是那一帧的真实延迟。
    v.push((
        "C214-缺测-缺测不被当成零延迟",
        s2.p50_us > 0,
    ));
    // 全缺测：分位无样本（不是 0延迟）
    let mut p3 = VirtioPerf::new();
    p3.submit_frame(frame(1, 5, 1000, 0, false));
    let s3 = p3.snapshot();
    v.push((
        "C214-缺测-全缺测时分位无样本",
        s3.latency_samples == 0,
    ));

    // 帧环形表定长：推超过 FRAME_SLOTS 帧，槽数不变
    let mut p4 = VirtioPerf::new();
    let mut i = 0;
    while i < FRAME_SLOTS * 3 {
        p4.submit_frame(frame(i as u64, 1, 1, 100, true));
        i += 1;
    }
    v.push((
        "C214-帧计-环形表定长不增长",
        p4.frames.len() == FRAME_SLOTS && p4.frames.pushed() == (FRAME_SLOTS * 3) as u64,
    ));
    v.push((
        "C214-帧计-越界读槽返回None",
        p4.frames.slot(FRAME_SLOTS).is_none() && p4.frames.slot(9999).is_none(),
    ));
    v
}

// ===========================================================================
// 四、快照队列（锚点「快照请求并发→串行化排队」「按需不常驻」）
// ===========================================================================

fn c214_queue() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 槽数对齐锚点
    v.push((
        "C214-排队-队列槽数对齐锚点",
        SNAP_QUEUE_SLOTS == ANCHOR_QUEUE_SLOTS,
    ));

    // 串行化 = FIFO：按入队序出队
    let mut q = SnapQueue::new();
    for t in 1..=5u64 {
        q.enqueue(t);
    }
    let mut order = Vec::new();
    while let Some(t) = q.dequeue() {
        order.push(t);
    }
    v.push((
        "C214-排队-出队顺序等于入队顺序",
        order.len() == 5 && order[0] == 1 && order[4] == 5,
    ));
    v.push((
        "C214-排队-空队列出队返None",
        q.dequeue().is_none() && q.len() == 0,
    ));

    // 满则拒收，**且不覆盖在队请求**（旧请求更早）。
    // 前置条件：队列必须**真的满**。先前版本在此之前先 dequeue 过一次，
    // 腾出一个空位后再要求「拒收」——前置不成立，判据必然红。
    // 教训（承 F0213）：写失败用例前先确认被测层的判定条件真的成立。
    let mut q2 = SnapQueue::new();
    let mut i = 0;
    while i < SNAP_QUEUE_SLOTS {
        q2.enqueue(i as u64);
        i += 1;
    }
    // 先确认它确实满了（否则下面两条是空断言）
    let full_now = q2.len() == SNAP_QUEUE_SLOTS;
    let rej1 = q2.enqueue(999);
    v.push((
        "C214-排队-满时拒收不覆盖",
        full_now && rej1 == QueueOutcome::RejectedFull && q2.len() == SNAP_QUEUE_SLOTS,
    ));
    v.push((
        "C214-排队-拒收计数递增",
        q2.rejected() == 1,
    ));
    // 拒收后仍按原序出队：0..8-1 一个不少，且 999 绝不在其中
    let mut ok_order = true;
    let mut j = 0;
    while let Some(t) = q2.dequeue() {
        if t != j as u64 {
            ok_order = false;
        }
        j += 1;
    }
    v.push((
        "C214-排队-拒收后仍按原序出队",
        ok_order && j == SNAP_QUEUE_SLOTS && q2.served() == SNAP_QUEUE_SLOTS as u64,
    ));

    // **不常驻**：构造 VirtioPerf 本身不产生快照；只在显式调用时组装
    let mut p = VirtioPerf::new();
    let built0 = p.snapshots_built;
    let _ = p.snapshot();
    v.push((
        "C214-快照-无请求零组装",
        built0 == 0,
    ));
    p.snapshot();
    v.push((
        "C214-快照-按需组装计数递增",
        p.snapshots_built == 2,
    ));
    // 快照带齐锚点要求的四类内容
    let s = p.snapshot();
    v.push((
        "C214-快照-四类指标齐备",
        s.frames_pushed == 0
            && s.total_commands.0 == 0
            && s.latency_samples == 0
            && s.telemetry_reported.len() == METRIC_FAMILIES,
    ));
    v
}

// ===========================================================================
// 五、遥测预算治理（锚点「计数入遥测总线受 F0096 预算治理」）
// ===========================================================================

fn c214_telemetry() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 阶梯对齐锚点
    v.push((
        "C214-预算-降采样阶梯对齐锚点",
        DOWN_STEPS == ANCHOR_STEPS,
    ));

    let mut t = TelemetrySink::new();
    // 100% 档：全采
    let mut all = 0;
    let mut i = 0;
    while i < 100 {
        if t.report(MetricFamily::Command) {
            all += 1;
        }
        i += 1;
    }
    v.push((
        "C214-预算-满档全采不丢点",
        all == 100 && t.dropped(MetricFamily::Command) == 0,
    ));

    // 降档：确实开始丢点（否则「降采样」是空动作）
    t.step_down();
    let rate = t.rate_pct();
    let mut taken = 0;
    i = 0;
    while i < 100 {
        if t.report(MetricFamily::Command) {
            taken += 1;
        }
        i += 1;
    }
    v.push((
        "C214-预算-降档后确实降采样",
        rate == ANCHOR_STEPS[1] && taken > 0 && taken < 100,
    ));
    // 对账：`submitted == reported + dropped`（三口径必须自洽，否则下游
    // 算采样率会得到 100% 的假象）。**只能有一条等式**——原先写成
    // `A + B - 100 == 0 || B == 100 - taken`，第一个分支在 u64 下会下溢
    // 成巨大值恒不成立，等于把真实对账藏进一个永不触发的分支里。
    let dropped_now = t.dropped(MetricFamily::Command);
    let reported_now = t.reported(MetricFamily::Command);
    let submitted_now = t.submitted(MetricFamily::Command);
    v.push((
        "C214-预算-降档丢弃计数对账",
        submitted_now == 200
            && reported_now == 100 + taken
            && dropped_now == 200 - reported_now,
    ));

    // 满档下 reported 必然等于 submitted（无丢弃）——两口径不得混用
    let mut t_full = TelemetrySink::new();
    let mut m = 0;
    while m < 10 {
        t_full.report(MetricFamily::Command);
        m += 1;
    }
    v.push((
        "C214-预算-满档采下数等于提交数",
        t_full.reported(MetricFamily::Command) == 10
            && t_full.submitted(MetricFamily::Command) == 10
            && t_full.dropped(MetricFamily::Command) == 0,
    ));

    // 阶梯回升（恢复后逐步升，不跳档）
    t.step_up();
    v.push((
        "C214-预算-升档逐级不跳",
        t.rate_pct() == ANCHOR_STEPS[0] && t.step() == 0,
    ));
    // 档位到底不越界
    let mut t2 = TelemetrySink::new();
    t2.step_up();
    v.push(("C214-预算-满档升档不越界", t2.step() == 0));
    let mut i2 = 0;
    while i2 < DOWN_STEPS.len() + 3 {
        t2.step_down();
        i2 += 1;
    }
    v.push((
        "C214-预算-降档到底不越界",
        t2.step() == DOWN_STEPS.len() - 1,
    ));

    // 配额超限检测（预算治理的触发条件）
    let mut t3 = TelemetrySink::new();
    t3.set_quota(MetricFamily::Byte, 5);
    let mut j = 0;
    while j < 10 {
        t3.report(MetricFamily::Byte);
        j += 1;
    }
    v.push((
        "C214-预算-超配额可检出",
        t3.over_quota(MetricFamily::Byte) && !t3.over_quota(MetricFamily::Command),
    ));
    v.push((
        "C214-预算-配额读回一致",
        t3.quota(MetricFamily::Byte) == 5 && t3.quota(MetricFamily::Command) == 0,
    ));

    // 优先级：延迟最高（丢不起）
    v.push((
        "C214-预算-延迟族优先级最高",
        MetricFamily::Latency.priority() > MetricFamily::Command.priority()
            && MetricFamily::Command.priority() > MetricFamily::Byte.priority(),
    ));
    v.push((
        "C214-预算-指标族名可判读",
        MetricFamily::Latency.name() != MetricFamily::Command.name()
            && MetricFamily::Command.index() < METRIC_FAMILIES,
    ));

    // 端到端：rebalance 在超配额时降档、未超时升档
    let mut p = VirtioPerf::new();
    p.telemetry.set_quota(MetricFamily::Command, 2);
    let mut k = 0;
    while k < 5 {
        p.telemetry.report(MetricFamily::Command);
        k += 1;
    }
    p.rebalance_telemetry();
    let down = p.telemetry.step();
    p.telemetry.set_quota(MetricFamily::Command, 10_000);
    p.rebalance_telemetry();
    v.push((
        "C214-预算-超配额降档未超升档",
        down == 1 && p.telemetry.step() == 0,
    ));
    v
}

// ===========================================================================
// 六、结构不变量（锚点「数据结构：计数器组×诊断快照」）
// ===========================================================================

fn c214_struct() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    v.push((
        "C214-结构-帧表长度常量对齐",
        FRAME_SLOTS == 64,
    ));
    v.push((
        "C214-结构-指标族数对齐",
        METRIC_FAMILIES == 3,
    ));
    // 分位单调性在真实多量级语料上已验；这里再验「快照分位单调」这条
    // **对外承诺**：下游 F0100 漂移检测要靠它做趋势比较
    let mut p = VirtioPerf::new();
    let mut i = 0;
    while i < 500 {
        p.submit_frame(frame(i as u64, 1, 1, 100, true));
        i += 1;
    }
    let mut p2 = VirtioPerf::new();
    let mut j = 0;
    while j < 50 {
        p2.submit_frame(frame(j as u64, 1, 1, 5000, true));
        j += 1;
    }
    let slow = p2.snapshot();
    let fast = p.snapshot();
    v.push((
        "C214-结构-快照分位随负载上移",
        slow.p99_us > fast.p99_us && slow.p50_us > fast.p50_us,
    ));
    v.push((
        "C214-结构-快照分位单调",
        slow.p50_us <= slow.p90_us && slow.p90_us <= slow.p99_us,
    ));

// --- 反假变体测试补的判据（11 个漏网变体逐条对应）---

    // M04：饱和后再累加仍改值。
    // 原判据只查「饱和后 add(999) 值不变」，但那是**同族**路径；
    // 变体把 `if self.saturated { return }` 改成 `if false` 后，
    // `add` 仍走 room 判定（value 已等于 SAT → room=0 → delta>0 仍钉住）
    // ⇒ 值碰巧不变，原判据恒真。须显式构造「饱和后 delta=0」——
    // 此时若饱和位被忽略，value 会不变但**语义已错**；故改为断言
    // 饱和位在饱和后任何 add（含 0）都不得影响 `snapshot()` 的双字段。
    let mut c_sat = SaturatedCounter::new();
    c_sat.add(COUNTER_SAT);
    c_sat.add(1); // 置饱和
    let sat_before = c_sat.snapshot();
    c_sat.add(0); // 零增量：饱和语义下应完全无副作用
    v.push((
        "C214-饱和-饱和后零增量无副作用",
        c_sat.snapshot() == sat_before && sat_before.1,
    ));

    // M05：reset 不清饱和位。
    // 原判据「正常累加」用的是**全新**计数器，reset 后不复查饱和位。
    let mut c_rst = SaturatedCounter::new();
    c_rst.add(u64::MAX);
    assert_sat(&mut c_rst);
    c_rst.reset();
    c_rst.add(7);
    v.push((
        "C214-饱和-reset后饱和位已清",
        c_rst.value() == 7 && !c_rst.is_saturated(),
    ));

    // M09：草图桶计数自身的饱和判断被去掉。
    // 原判据只查「桶计数总和 == 样本数」，样本数远小于 u32::MAX，
    // 去掉判断与保留判断在该规模下**完全同值**⇒恒真弱门禁。
    // 判别办法：桶 i 的计数恰等于 u32::MAX 时，再打一点**不得溢出回绕**
    // （回绕成 0 会让「桶计数总和 == 样本数」失真，且分位指向错桶）。
    let mut s_sat = QuantileSketch::new();
    // 灌到顶桶：桶 0 覆盖 [1,2)，直接 record(u32::MAX as u64) 落顶桶不划算，
    // 改灌桶 0（record(1)），灌满 u32::MAX 次成本太高 ⇒ 用 record 循环
    // 灌到接近饱和不可行（4e9 次）。故换判别面：**桶计数不得回绕**，
    // 用桶 15（顶桶）少量样本验证「跨桶不串扰」+ 桶计数为 u32 而非更窄。
    let mut i15 = 0;
    while i15 < 3 {
        s_sat.record(1u64 << 20); // 落桶 20 → 被钳到顶桶 15
        i15 += 1;
    }
    let top = s_sat.bucket(SKETCH_MAX_SHIFT as usize);
    v.push((
        "C214-分位-超量级样本钳到顶桶不丢",
        top == 3 && s_sat.count() == 3 && s_sat.bucket(20) == 0,
    ));
    // 顶桶计数用 u32：连续打点至 u32::MAX 不可行（成本），
    // 改断言「桶计数类型宽度」——通过「桶计数总和 == 样本数」在
    // 大样本下成立来间接保证不溢出（变体 M09 去掉判断后仍是同值，
    // 故这条判据只能证明当前规模下无差异；真正的溢出防护由
    // `record` 的 `if buckets[idx] < u32::MAX` 承担，本判据守的是
    // 「钳位后不丢样本」这一可观测行为）。

    // M11：越界读桶返越界值而非 0。
    // 原判据 `s.bucket(SKETCH_BUCKETS) == 0 && s.bucket(9999) == 0`
    // 用的是**空草图附近的 s**（只 record(100)），越界索引回绕到
    // `min(idx, 15)` 时恰好落在空桶 15 → 与正确实现**同值**。
    // 修：读越界前先把**所有**桶灌非零，回绕后必现非零 ⇒ 有判别力。
    let mut s_all = QuantileSketch::new();
    let mut kk = 0;
    while kk < SKETCH_BUCKETS {
        s_all.record(1u64 << kk);
        kk += 1;
    }
    v.push((
        "C214-分位-越界读桶在全满草图上仍返0",
        s_all.bucket(SKETCH_BUCKETS) == 0
            && s_all.bucket(9999) == 0
            && s_all.bucket(usize::MAX) == 0
            && s_all.occupied_buckets() == SKETCH_BUCKETS,
    ));

    // M13：缺测不清 measured 位。
    // 原判据 `s2.frames_missing == 1 && s2.frames_measured == 1` 中，
    // `mark_missing` 若不清 measured，`FrameRing::push` 会把它记成
    // measured ⇒ frames_measured 变 2 ⇒ 该判据应转红……但
    // `frame()` 辅助里缺测分支走 `mark_missing()`，
    // 而 `submit_frame` 又按 `fc.measured` 决定是否进分位，
    // 两处都被同一个 bool 驱动 ⇒ **同源恒真**。
    // 判别面：直接断言 `mark_missing()` 后 `measured` 字段本身为 false
    // （不经 FrameRing 聚合，消除聚合掩盖）。
    let mut f_miss = FrameCounters::new(1);
    f_miss.mark_submit();
    f_miss.mark_missing();
    v.push((
        "C214-缺测-mark_missing直接清measured位",
        f_miss.missing && !f_miss.measured,
    ));

    // M16：latest 不随 next 回绕（读错帧）。
    // 原判据无此项。补：绕回一圈后 latest 必须是**真正 newest**，
    // 且 `latest` 在空表返 None。
    let mut r_new = FrameRing::new();
    v.push(("C214-帧计-空表latest返None", r_new.latest().is_none()));
    let mut q1 = 0;
    while q1 < FRAME_SLOTS {
        r_new.push(FrameCounters::new(q1 as u64));
        q1 += 1;
    }
    // 此刻 next 绕回 0，latest 应是最后一推的 FRAME_SLOTS-1
    let wrap_ok = r_new.latest().map(|f| f.frame_id) == Some((FRAME_SLOTS - 1) as u64);
    r_new.push(FrameCounters::new(9999));
    v.push((
        "C214-帧计-latest绕回后仍指最新帧",
        wrap_ok && r_new.latest().map(|f| f.frame_id) == Some(9999),
    ));

    // M23：抽样序号取自 reported 而非 submitted。
    // 原判据 `降档后确实降采样` 只查 taken<100，在 50% 档下
    // 「取自 reported」也会得到 50% ⇒ 同值恒真。
    // 判别面：**10% 档**。取自 submitted 时每 10 采 1（100 点采 10）；
    // 取自 reported 时 reported 增长慢 ⇒ 退化，100 点会采更多。
    let mut t_dec = TelemetrySink::new();
    t_dec.step_down();
    t_dec.step_down(); // 10%
    let mut taken10 = 0;
    let mut q3 = 0;
    while q3 < 100 {
        if t_dec.report(MetricFamily::Command) {
            taken10 += 1;
        }
        q3 += 1;
    }
    v.push((
        "C214-预算-十分档按提交序抽样",
        t_dec.rate_pct() == ANCHOR_STEPS[2]
            && taken10 == 10
            && t_dec.submitted(MetricFamily::Command) == 100,
    ));

    // M27：超配额判定用 >= 而非 >（边界错判）。
    // 原判据超配额 10 点 vs 配额 5，远大于边界 ⇒ >= 与 > 同值。
    // 判别面：**恰好等于配额**时不得判超限。
    let mut t_edge = TelemetrySink::new();
    t_edge.set_quota(MetricFamily::Command, 10);
    let mut q4 = 0;
    while q4 < 10 {
        t_edge.report(MetricFamily::Command);
        q4 += 1;
    }
    let at_edge = t_edge.over_quota(MetricFamily::Command);
    t_edge.report(MetricFamily::Command);
    let past_edge = t_edge.over_quota(MetricFamily::Command);
    v.push((
        "C214-预算-恰好等于配额不判超限",
        !at_edge && past_edge,
    ));

    // M29：缺测帧也进跨帧分位（缺测记 0 当零延迟）。
    // 原判据 `缺测不产生延迟样本` 用 `frame(2,5,1000,0,false)`，
    // 而 `frame()` 的缺测分支根本不调 `latency.record` ⇒ 帧内草图
    // 本就无样本，变体把 `if fc.measured` 改成 `if true` 后，
    // 从**空草图**回灌仍是 0 个样本 ⇒ 同值恒真。
    // 判别面：**手工构造一个「帧内草图非空但 measured=false」的帧**
    // （模拟上游在缺测前已打了点），此时 submit_frame 若收它，
    // 跨帧样本数会从 1 涨到 2。
    let mut p_mix = VirtioPerf::new();
    p_mix.submit_frame(frame(1, 1, 1, 5000, true));
    let mut f_dirty = FrameCounters::new(2);
    f_dirty.add_commands(1);
    f_dirty.latency.record(100); // 帧内有样本…
    f_dirty.mark_missing();     // …但整帧被判缺测
    p_mix.submit_frame(f_dirty);
    let s_mix = p_mix.snapshot();
    v.push((
        "C214-缺测-缺测帧不入跨帧分位",
        s_mix.latency_samples == 1,
    ));

    // M31：serve_one 空队列虚报服务。
    // 原判据 `按需组装` 等只测快照；`serve_one` 无独立判据。
    let mut p_idle = VirtioPerf::new();
    v.push((
        "C214-快照-空队列不虚报服务",
        p_idle.serve_one() == QueueOutcome::Idle
            && !QueueOutcome::Idle.did_serve()
            && QueueOutcome::Served.did_serve(),
    ));

    // M33：跨帧累计绕过饱和语义。
    // 原判据无此项。补：两帧累计恰好把总量顶到饱和线以上，
    // 必须置饱和位——若绕过（如直接相加且恒 `saturated: false`）则转红。
    let mut p_sat = VirtioPerf::new();
    p_sat.submit_frame(frame(1, COUNTER_SAT, COUNTER_SAT, 100, true));
    p_sat.submit_frame(frame(2, 1, 1, 100, true));
    let s_sat2 = p_sat.snapshot();
    v.push((
        "C214-帧计-跨帧累计溢出置饱和位",
        s_sat2.total_commands.0 == COUNTER_SAT
            && s_sat2.total_commands.1
            && s_sat2.total_bytes.0 == COUNTER_SAT
            && s_sat2.total_bytes.1,
    ));
    v
}

/// 辅助：确保计数器已置饱和（供 reset 判据的前置条件构造）。
fn assert_sat(c: &mut SaturatedCounter) {
    c.add(COUNTER_SAT);
    c.add(1);
}

/// 跑完 VE-F0214 全部判据。
pub fn run_veb14_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb14");
    for group in [c214_saturated, c214_sketch, c214_frames, c214_queue, c214_telemetry, c214_struct] {
        for (name, passed) in group() {
            set.add(&name, passed, "");
        }
    }
    set
}
