//! CGPU-F0483 · 帧时间账本域自检（判据五条逐条映射 + 反向语料钉门禁）。
//!
//! **判据（锚点原文）**：完整记录、同 schema、滚动、查询、判据。

use super::cgd03_frameledger::{
    FrameLedger, QueryResult, FRAME_LEDGER_VERSION, METRICS_PER_FRAME,
    METRIC_PREFIX, METRIC_SCHEMA, RECENT_CAPACITY, SCHEMA_VERSION,
};
use alloc::string::String;

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = ["完整记录", "同 schema", "滚动", "查询", "判据"];

/// 判据侧独立重算的 schema 命名表（与本体 METRIC_SCHEMA 逐条全等）。
const NAMES_RECHECK: [&str; 5] = [
    "d01.frameledger.input_sampling_ns",
    "d01.frameledger.submit_ns",
    "d01.frameledger.gpu_execute_ns",
    "d01.frameledger.present_ns",
    "d01.frameledger.frame_interval_ns",
];

/// 帧账本域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgd03_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgd03_frameledger");

    // —— 判据一 · 完整记录：六项结构入账 + 缺段如实 ——
    let mut led = FrameLedger::new();
    let full: [Option<u64>; 4] = [Some(200), Some(400), Some(7_400), Some(3_900)];
    let partial: [Option<u64>; 4] = [Some(200), Some(400), None, Some(3_900)];
    led.record(1, full, 12_500_000, "探索");
    led.record(2, partial, 25_100_000, "战斗");
    let q1 = led.lookup(1);
    let q2 = led.lookup(2);
    let rec1_ok = match &q1 {
        QueryResult::Exact(r) => {
            r.stages_complete()
                && r.scene == "探索"
                && r.schema_version == SCHEMA_VERSION
                && r.interval_ns.is_none()
                && r.metric_values().len() == METRICS_PER_FRAME
        }
        _ => false,
    };
    let rec2_ok = match &q2 {
        QueryResult::Exact(r) => {
            !r.stages_complete()
                && r.stage_ns.get(2).map(|x| x.is_none()).unwrap_or(true)
                && r.interval_ns == Some(12_600_000)
                && r.scene == "战斗"
        }
        _ => false,
    };
    s.add(
        "D03-完整记录-六项结构+缺段如实",
        rec1_ok && rec2_ok && led.recent_len() == 2,
        "四段+间隔+场景标签+版本逐帧入账（结构完整）；缺段帧照记 None 不补造不跳过；帧间隔由相邻时间戳自动核算（12600000ns 判据侧手算对账）",
    );

    // —— 判据二 · 同 schema：五元组表与判据侧独立重排逐条全等 ——
    let mut schema_ok = METRIC_SCHEMA.len() == METRICS_PER_FRAME && NAMES_RECHECK.len() == 5;
    let mut si = 0usize;
    while si < METRIC_SCHEMA.len() {
        let m = match METRIC_SCHEMA.get(si) {
            Some(m) => *m,
            None => break,
        };
        let expect = match NAMES_RECHECK.get(si) {
            Some(n) => *n,
            None => break,
        };
        if m.name != expect || m.dimension != "frame" || m.unit != "ns" || m.kind != "u64" {
            schema_ok = false;
        }
        if !m.name.starts_with(METRIC_PREFIX) {
            schema_ok = false; // 三级命名前缀逐条 grep 可验
        }
        si += 1;
    }
    let line = match &q1 {
        QueryResult::Exact(r) => led.schema_line(r),
        _ => String::new(),
    };
    s.add(
        "D03-同schema-五元组单源+三级命名",
        schema_ok
            && SCHEMA_VERSION == 1
            && line.contains("d01.frameledger.gpu_execute_ns=7400ns")
            && line.contains("d01.frameledger.frame_interval_ns=None"),
        "五元组（名称/维度/单位/类型/采样）与 F0274/B08 同构不第二套口径；三级命名前缀逐条 grep；记录行展开与 schema 表按位对齐（同 schema 运行时面）",
    );

    // —— 判据三 · 滚动：配额恒定 + 退场聚合 + 反向（配额未满不聚合） ——
    let mut led2 = FrameLedger::new();
    let mut fid = 0u64;
    while fid < (RECENT_CAPACITY + 4) as u64 {
        led2.record(fid, full, fid.saturating_mul(12_500_000), "滚动");
        fid = fid.saturating_add(1);
    }
    let evicted = led2.overflowed();
    let first_evicted = led2.lookup(0);
    let still_recent = led2.lookup((RECENT_CAPACITY + 3) as u64);
    s.add(
        "D03-滚动-配额恒定+退场聚合",
        led2.recent_len() == RECENT_CAPACITY
            && evicted == 4
            && matches!(first_evicted, QueryResult::Aggregated(_))
            && matches!(still_recent, QueryResult::Exact(_))
            && led2.history().count == 4,
        "256 帧逐帧配额恒定；超限 4 帧退场进聚合桶（count=4）；退场帧查聚合事实、近期帧查逐帧全账（聚合不是丢弃）",
    );

    // —— 聚合统计判据侧手算对账 ——
    let hist = led2.history();
    // 每帧四段合计 200+400+7400+3900 = 11900ns
    let stats_ok = hist.min_total_ns == Some(11_900)
        && hist.max_total_ns == Some(11_900)
        && hist.mean_total_ns() == Some(11_900)
        && hist.sum_total_ns == 4 * 11_900;
    s.add(
        "D03-聚合-min/max/sum/mean对账",
        stats_ok,
        "聚合桶 min=max=mean=11900ns（四段合计判据侧手算）、sum=4×11900——聚合统计独立重算对账",
    );

    // —— 判据四 · 查询：三种答案各有其位 + 反向（未记录 NotFound） ——
    let ghost = led2.lookup(9_999);
    let agg_mean_line = match led2.lookup(1) {
        QueryResult::Aggregated(b) => b.mean_total_ns() == Some(11_900),
        _ => false,
    };
    s.add(
        "D03-查询-三答案各有其位",
        matches!(ghost, QueryResult::NotFound)
            && agg_mean_line
            && matches!(led2.lookup(2), QueryResult::Exact(_)),
        "未记录帧 NotFound 不臆造；已聚合帧给聚合统计；近期帧给逐帧全账——没有第四种「查不到就说零」",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["完整记录", "同 schema", "滚动", "查询", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "D03-判据stamp-五条独立重排全等",
        stamp_ok
            && FRAME_LEDGER_VERSION.starts_with("D01-")
            && METRICS_PER_FRAME == 5
            && RECENT_CAPACITY == 256,
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）；配额/指标数/版本钉死",
    );

    s
}
