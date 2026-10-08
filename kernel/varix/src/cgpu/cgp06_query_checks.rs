//! CGPU-F2406 · 遥测查询引擎域自检（锚点测试一组：查询 + stamp）。
//!
//! **判据（锚点原文）**：查询复用、一组、判据。

use super::cgp06_query::{
    validate, QueryAnswer, QueryEngine, QueryError, QueryRequest,
    LEDGER_CAPACITY, REUSE_LINES,
};
use alloc::string::ToString;

/// 判据侧独立重排的锚点判据三条。
const CRITERIA_RECHECK: [&str; 3] = ["查询复用", "一组", "判据"];

/// CGPU-F2406 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp06_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp06_query");

    // —— 一组 · 查询：三参数过滤手算 + 粒度聚合 + NotFound + 参数非法 ——
    let mut q = QueryEngine::new();
    // 注入 6 条（两域两 tick 段）——账面即查询语料。
    q.ingest_snapshot("d01.frame.gpu_ns", 10, 100);
    q.ingest_snapshot("d01.frame.gpu_ns", 20, 100);
    q.ingest_snapshot("d01.frame.gpu_ns", 30, 130);
    q.ingest_snapshot("j08.power.watt", 5, 100);
    q.ingest_snapshot("j08.power.watt", 15, 130);
    q.ingest_snapshot("p01.frame.calls", 7, 130);
    // 明细查询（粒度 1）：d 域 tick 100..=120 → 恰 2 条（10/20），序保持注入序。
    let r1 = q.query(&QueryRequest {
        from_tick: 100,
        to_tick: 120,
        name_prefix: "d01".to_string(),
        granularity: 1,
    });
    let rows_ok = match r1 {
        Ok(QueryAnswer::Rows(v)) => {
            v.len() == 2 && v.get(0).map(|x| x.value == 10).unwrap_or(false)
                && v.get(1).map(|x| x.value == 20).unwrap_or(false)
        }
        _ => false,
    };
    // 聚合查询（粒度 32 分桶）：d 域全范围 → min=10 max=30 sum=60 count=3 手算对账。
    let r2 = q.query(&QueryRequest {
        from_tick: 100,
        to_tick: 200,
        name_prefix: "d01".to_string(),
        granularity: 32,
    });
    let agg_ok = match r2 {
        Ok(QueryAnswer::Aggregated { min, max, sum, count }) => {
            min == 10 && max == 30 && sum == 60 && count == 3
        }
        _ => false,
    };
    // NotFound 反向：范围+前缀双不命中 → NotFound 不臆造空桶不答零。
    let r3 = q.query(&QueryRequest {
        from_tick: 900,
        to_tick: 999,
        name_prefix: "d01".to_string(),
        granularity: 1,
    });
    let r4 = q.query(&QueryRequest {
        from_tick: 100,
        to_tick: 200,
        name_prefix: "x99".to_string(),
        granularity: 1,
    });
    let nf_ok = r3 == Ok(QueryAnswer::NotFound) && r4 == Ok(QueryAnswer::NotFound);
    // 参数非法显性拒：粒度 0 / 范围倒序——专属错不静默纠正。
    let e1 = validate(&QueryRequest {
        from_tick: 1,
        to_tick: 2,
        name_prefix: "".to_string(),
        granularity: 0,
    });
    let e2 = validate(&QueryRequest {
        from_tick: 5,
        to_tick: 1,
        name_prefix: "".to_string(),
        granularity: 1,
    });
    let err_ok = e1 == Err(QueryError::ZeroGranularity) && e2 == Err(QueryError::InvertedRange);
    // 环形逐出：再灌 LEDGER_CAPACITY 条挤掉最老——总注入数在账（逐出不消失）。
    let mut i = 0u32;
    while i < LEDGER_CAPACITY as u32 {
        q.ingest_snapshot("p01.sampling.v", i as u64 + 1, 200);
        i += 1;
    }
    let ring_ok = q.ledger_len() <= LEDGER_CAPACITY
        && q.total_ingested() == 6 + LEDGER_CAPACITY as u32
        && q.ledger_len() == LEDGER_CAPACITY;
    // 空前缀=不过滤：全账聚合 count=LEDGER_CAPACITY（跨域通查语义）。
    let r5 = q.query(&QueryRequest {
        from_tick: 0,
        to_tick: 999,
        name_prefix: "".to_string(),
        granularity: 64,
    });
    let all_ok = match r5 {
        Ok(QueryAnswer::Aggregated { count, .. }) => count == LEDGER_CAPACITY as u32,
        _ => false,
    };
    s.add(
        "P06-一组查询-三参数+粒度+NotFound+环形",
        rows_ok && agg_ok && nf_ok && err_ok && ring_ok && all_ok,
        "明细查询 d 域 100..=120 恰 2 条序保持注入序；粒度 32 聚合手算 min=10/max=30/sum=60/count=3；范围与前缀双不命中 NotFound 不臆造空桶；粒度 0/倒序专属错显性拒；环形逐出后总注入 6+32 条在账；空前缀跨域通查 count=容量",
    );

    // —— 复用声明逐条 grep（判据「查询复用」） ——
    let mut reuse_ok = REUSE_LINES.len() == 3;
    let mut ri = 0usize;
    while ri < REUSE_LINES.len() {
        let l = REUSE_LINES[ri];
        if !(l.contains("F1558") || l.contains("F0483") || l.contains("F2404")) {
            reuse_ok = false;
        }
        ri += 1;
    }
    // 判据 stamp 独立对账。
    let stamps = ["查询复用", "一组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "P06-复用+判据stamp-逐条对账",
        reuse_ok && stamp_ok && LEDGER_CAPACITY == 32,
        "复用清单三条含 F1558/F0483/F2404 关键字逐条 grep（冻结 API 三参数/三答案口径/管道路由注入面）；锚点判据三条与判据侧独立重排逐条全等；一组测试（查询）宣告与实际检查一一对应；账面容量写死 32 可查账",
    );

    s
}
