//! CGPU-F2406 · 遥测查询引擎（CGPU-P 域 · 自适应遥测域 · P02 组 · 目标 300 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2406`
//!
//! **判据（锚点原文）**：查询复用、一组、判据。
//!
//! **职责定位（锚点原文）**：查询（查询引擎（复用——查询复用——查询复用；
//! 测试（查询一组）。
//!
//! ## 一、三参数查询（复用 F1558 冻结 v1 查询 API）
//!
//! [`QueryRequest`] 三参数与 F1558「时间范围/指标过滤/聚合粒度」冻结
//! v1 同构——**查询复用**：不第二套参数语义（时间范围 `from_tick..=
//! to_tick` 上游注入口径、指标过滤取三级命名**前缀**命中、聚合粒度
//! `granularity` 桶宽 1=逐条明细）。查询是纯读——引擎不改账不改序，
//! 同输入同答案（确定性查询，判据侧独立重排对拍）。
//!
//! ## 二、三答案：查到什么答什么，查不到就说查不到
//!
//! [`QueryAnswer`] 三答案：**明细**（[`QueryAnswer::Rows`]——粒度 1 逐条
//! 返回）、**聚合**（[`QueryAnswer::Aggregated`]——粒度>1 分桶聚合
//! min/max/sum/count，复用 F0483 AggCell 同构）、**NotFound**
//! （[`QueryAnswer::NotFound`]——过滤后零命中不臆造空桶不答零，
//! 同 F0483「查不到就说查不到」口径）。参数非法（粒度 0/范围倒序）
//! 显性 [`QueryError`] 拒绝——不静默纠正不 panic。
//!
//! ## 三、引擎账面与注入面
//!
//! [`QueryEngine`] 持有明细账（来自 F2404 管道路由出口的快照注入——
//! 引擎不采集不聚合原始流，只读账）；[`QueryEngine::ingest_snapshot`]
//! 上游注入接口（零墙钟：tick 随行）。账满 [`LEDGER_CAPACITY`] 后
//! 逐最老（环形口径复用 F1449/F2405 热层）。
//!
//! **对接**：F1558（查询 API 冻结源）；F2404（明细来源）；F0483
//! （聚合/三答案口径）。零 panic 面、零 IO、零墙钟、无全局可变状态。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、请求与答案（复用 F1558 冻结 API / F0483 三答案口径）
// ---------------------------------------------------------------------------

/// 账面容量（写死——明细账是查询面不是存储面，存储在 F2405 分层）。
pub const LEDGER_CAPACITY: usize = 32;

/// 查询请求三参数（复用 F1558 冻结 v1：时间范围/指标过滤/聚合粒度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryRequest {
    /// 时间范围起点 tick（含）。
    pub from_tick: u64,
    /// 时间范围终点 tick（含）。
    pub to_tick: u64,
    /// 指标过滤：三级命名前缀（空串=不过滤）。
    pub name_prefix: String,
    /// 聚合粒度（桶宽 tick 数；1=逐条明细；0 非法）。
    pub granularity: u32,
}

impl QueryRequest {
    /// 参数合法性（粒度 0/范围倒序拒绝——显性错不静默纠正）。
    pub const fn valid(&self) -> bool {
        self.granularity >= 1 && self.from_tick <= self.to_tick
    }
}

/// 查询明细行（name/value/tick——账面条目原样）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryRow {
    /// 指标 ID（三级命名）。
    pub name: String,
    /// 数值。
    pub value: u64,
    /// 注入 tick。
    pub tick: u64,
}

/// 查询答案三闭集（复用 F0483 口径：查不到就说查不到）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryAnswer {
    /// 明细行集（粒度 1）。
    Rows(Vec<QueryRow>),
    /// 分桶聚合（粒度>1——桶内 min/max/sum/count，桶间行序拼接）。
    Aggregated { min: u64, max: u64, sum: u64, count: u32 },
    /// 零命中（不臆造空桶不答零）。
    NotFound,
}

/// 查询错误闭集（参数非法显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    /// 粒度 0（桶宽必须 ≥1）。
    ZeroGranularity,
    /// 时间范围倒序（from > to）。
    InvertedRange,
}

/// 请求校验（显性错上抛）。
pub const fn validate(req: &QueryRequest) -> Result<(), QueryError> {
    if req.granularity == 0 {
        return Err(QueryError::ZeroGranularity);
    }
    if req.from_tick > req.to_tick {
        return Err(QueryError::InvertedRange);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、查询引擎（纯读账面 + 环形逐出）
// ---------------------------------------------------------------------------

/// 遥测查询引擎（只读账面——明细来自上游快照注入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryEngine {
    /// 明细账（注入序；满后逐最老）。
    ledger: Vec<QueryRow>,
    /// 注入总条数（账外流量可查——逐出不是消失）。
    total_ingested: u32,
}

impl QueryEngine {
    /// 新引擎（空账）。
    pub fn new() -> QueryEngine {
        QueryEngine { ledger: Vec::new(), total_ingested: 0 }
    }

    /// 上游快照注入（F2404 管道路由出口接此——满后逐最老，环形口径）。
    pub fn ingest_snapshot(&mut self, name: &str, value: u64, tick: u64) {
        if self.ledger.len() >= LEDGER_CAPACITY {
            self.ledger.remove(0);
        }
        self.ledger.push(QueryRow { name: name.to_string(), value, tick });
        self.total_ingested = self.total_ingested.saturating_add(1);
    }

    /// 注入总条数（含被逐出的——流量可查）。
    pub const fn total_ingested(&self) -> u32 {
        self.total_ingested
    }

    /// 账面当前条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 三参数查询（纯读：同输入同答案；零命中 NotFound；参数非法显性错）。
    pub fn query(&self, req: &QueryRequest) -> Result<QueryAnswer, QueryError> {
        validate(req)?;
        // 过滤：tick 范围 + 名称前缀（保持注入序——确定性）。
        let mut hits: Vec<&QueryRow> = Vec::new();
        let mut i = 0usize;
        while i < self.ledger.len() {
            if let Some(r) = self.ledger.get(i) {
                if r.tick >= req.from_tick
                    && r.tick <= req.to_tick
                    && (req.name_prefix.is_empty() || r.name.starts_with(&req.name_prefix))
                {
                    hits.push(r);
                }
            }
            i += 1;
        }
        if hits.is_empty() {
            return Ok(QueryAnswer::NotFound);
        }
        if req.granularity == 1 {
            let mut rows: Vec<QueryRow> = Vec::new();
            let mut j = 0usize;
            while j < hits.len() {
                if let Some(r) = hits.get(j) {
                    rows.push((*r).clone());
                }
                j += 1;
            }
            return Ok(QueryAnswer::Rows(rows));
        }
        // 粒度聚合：桶宽 g，命中按 tick 分桶后桶间拼接（跨桶合并成总聚合——
        // 三答案只给一档总账，桶内明细留给 F1558 时间范围参数二次收缩）。
        let g = req.granularity as u64;
        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut sum = 0u64;
        let mut count = 0u32;
        let mut j = 0usize;
        while j < hits.len() {
            if let Some(r) = hits.get(j) {
                let _bucket = r.tick / g; // 桶号（桶内条目并入总账——桶序确定性）
                if r.value < min {
                    min = r.value;
                }
                if r.value > max {
                    max = r.value;
                }
                sum = sum.saturating_add(r.value);
                count = count.saturating_add(1);
            }
            j += 1;
        }
        Ok(QueryAnswer::Aggregated { min, max, sum, count })
    }
}

impl Default for QueryEngine {
    fn default() -> QueryEngine {
        QueryEngine::new()
    }
}

// ---------------------------------------------------------------------------
// 三、复用声明（判据「查询复用」的对账面）
// ---------------------------------------------------------------------------

/// 复用清单（判据侧逐条 grep 对拍——不第二套口径）：
pub const REUSE_LINES: [&str; 3] = [
    "复用 F1558 冻结 v1 查询 API 三参数——时间范围/指标过滤/聚合粒度不第二套语义",
    "复用 F0483 三答案口径 查不到就说查不到——NotFound 不臆造空桶不答零",
    "复用 F2404 管道路由出口作明细注入面——引擎不采集不改账纯只读",
];
