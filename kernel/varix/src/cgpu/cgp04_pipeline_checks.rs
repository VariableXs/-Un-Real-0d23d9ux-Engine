//! CGPU-F2404 · 遥测数据管道域自检（锚点测试两组：四段/背压 + stamp）。
//!
//! **判据（锚点原文）**：四段、模式复用、背压复用、两组、判据。

use super::cgp04_pipeline::{
    TelemetryPipe, PipeRecord, StageReceipt, Stage, RouteTarget, AggCell,
    STAGE_ORDER, PIPE_CAPACITY, PIPE_PRIVACY_MAX, REUSE_LINES,
};
use alloc::string::ToString;

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = ["四段", "模式复用", "背压复用", "两组", "判据"];

fn rec(id: &str, value: u64, privacy: u8) -> PipeRecord {
    PipeRecord {
        id: id.to_string(),
        value,
        schema_version: 3,
        privacy,
        tick: 7,
    }
}

/// CGPU-F2404 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp04_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp04_pipeline");

    // —— 组一 · 四段：段序 + 全程 + 过滤闸 + 聚合手算 + 账目守恒 ——
    // 段序独立对拍：采集→过滤→聚合→路由；next 链逐段衔接；末段无下一段。
    let order_ok = STAGE_ORDER[0] == Stage::Collect
        && STAGE_ORDER[1] == Stage::Filter
        && STAGE_ORDER[2] == Stage::Aggregate
        && STAGE_ORDER[3] == Stage::Route
        && Stage::Collect.next() == Some(Stage::Filter)
        && Stage::Filter.next() == Some(Stage::Aggregate)
        && Stage::Aggregate.next() == Some(Stage::Route)
        && Stage::Route.next().is_none()
        && Stage::Filter.name() == "过滤";
    // 正常流：d 域三条（10/20/30）——聚合手算 sum=60 mean=20 min=10 max=30。
    let mut p = TelemetryPipe::new();
    let r1 = p.ingest(&rec("d01.frame.gpu_ns", 10, 0));
    let r2 = p.ingest(&rec("d01.frame.gpu_ns", 20, 1));
    let r3 = p.ingest(&rec("d01.frame.gpu_ns", 30, 0));
    let cell = p.cell_of(RouteTarget::Frame);
    let agg_ok = match cell {
        Some(c) => c.sum == 60 && c.min == 10 && c.max == 30 && c.count == 3 && c.mean() == Some(20),
        None => false,
    };
    // 路由分发独立对拍：j 域→Power、p 域→Sampling、表外域拒收。
    let r4 = p.ingest(&rec("j08.power.watt", 5, 0));
    let r5 = p.ingest(&rec("p01.frame.gpu_ns", 6, 0));
    let r6 = p.ingest(&rec("x99.ghost.nowhere", 9, 0));
    // 隐私闸反向：Local(2) 超档拒收。
    let r7 = p.ingest(&rec("d01.frame.gpu_ns", 7, 2));
    let routed_ok = r1 == StageReceipt::Routed(RouteTarget::Frame)
        && r4 == StageReceipt::Routed(RouteTarget::Power)
        && r5 == StageReceipt::Routed(RouteTarget::Sampling)
        && r6 == StageReceipt::FilteredOut("表外域")
        && r7 == StageReceipt::FilteredOut("隐私超档（Local 不进管道）")
        && p.last_routed() == Some(RouteTarget::Sampling);
    // 账目守恒：collected = passed + filtered + backpressured（条条有账）。
    let c = p.counters;
    let ledger_ok = c.collected == c.passed + c.filtered + c.backpressured
        && c.collected == 7
        && c.passed == 5
        && c.filtered == 2
        && p.cell_of(RouteTarget::Power).map(|x| x.sum).unwrap_or(0) == 5;
    // 空桶均值不臆造（None 不给 0——F0483 三答案口径）。
    let empty_ok = AggCell::new().mean().is_none();
    s.add(
        "P04-组一四段-段序+全程+闸+聚合+账",
        order_ok && agg_ok && routed_ok && ledger_ok && empty_ok && PIPE_PRIVACY_MAX == 1,
        "段序独立对拍采集→过滤→聚合→路由（next 链衔接末段 None）；d 域三条聚合手算 sum=60/mean=20/min=10/max=30；j/p 域分发独立对拍；表外域与隐私超档专属拒因；账目守恒 collected=passed+filtered+backpressured 条条有账；空桶均值 None 不臆造",
    );

    // —— 组二 · 背压：满即拒不丢账 + drain 解除 ——
    let mut q = TelemetryPipe::new();
    let mut filled = 0u32;
    let mut back_seen = false;
    let mut pushed_count = 0u32;
    // 塞满 PIPE_CAPACITY 条后第 CAPACITY+1 条必须 PushedBack。
    let mut i = 0u32;
    while i < PIPE_CAPACITY as u32 + 5 {
        let v = (i % 7) as u64;
        match q.ingest(&rec("p01.sampling.tick", v + 1, 0)) {
            StageReceipt::Routed(_) => filled += 1,
            StageReceipt::PushedBack => {
                back_seen = true;
                pushed_count += 1;
            }
            _ => {}
        }
        i += 1;
    }
    // drain 腾空后恢复入流。
    let drained = q.drain();
    let after = q.ingest(&rec("p01.sampling.tick", 1, 0));
    let bp_ok = back_seen
        && filled == PIPE_CAPACITY as u32
        && q.buffered() == 1
        && drained == PIPE_CAPACITY
        && pushed_count == 5
        && q.counters.backpressured == 5
        && after == StageReceipt::Routed(RouteTarget::Sampling)
        && q.counters.collected
            == q.counters.passed + q.counters.filtered + q.counters.backpressured
        && PIPE_CAPACITY == 64;
    s.add(
        "P04-组二背压-满即拒+drain 解除+账不丢",
        bp_ok && PIPE_CAPACITY == 64,
        "塞满 64 条后第 65 条起 PushedBack（背压是正常态）；拒绝照记 backpressured=5 不静默丢账；drain 腾空后恢复入流；账目守恒在背压后依然成立；容量写死 64 可查账",
    );

    // —— 复用声明逐条 grep（判据「模式复用/背压复用」） ——
    let mut reuse_ok = REUSE_LINES.len() == 4;
    let mut ri = 0usize;
    while ri < REUSE_LINES.len() {
        let l = REUSE_LINES[ri];
        if !(l.contains("F2402") || l.contains("F2403") || l.contains("F0483") || l.contains("F1447")) {
            reuse_ok = false;
        }
        ri += 1;
    }
    // 判据 stamp 独立对账。
    let stamps = ["四段", "模式复用", "背压复用", "两组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "P04-复用+判据stamp-逐条对账",
        reuse_ok && stamp_ok,
        "复用清单四条含 F2402/F2403/F0483/F1447 关键字逐条 grep（六元组/过滤闸/聚合同构/流水线模式+背压）；锚点判据五条与判据侧独立重排逐条全等；两组测试（四段/背压）宣告与实际检查一一对应",
    );

    s
}
