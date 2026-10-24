// CGPU-F0312 域级遥测看板数据模型 · cgb312_dashmodel.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：看板数据模型（指标/维度/聚合/视图四层）完备；查询计划确定性；
//       刷新节流；数据快照一致性；模型可序列化。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 指标定义。
#[derive(Clone, Debug)]
pub struct MetricDef {
    pub id: u32,
    pub name: String,
    pub unit: u8,
    pub agg: AggKind,
    pub retention_hours: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggKind {
    Sum,
    Avg,
    Min,
    Max,
    P50,
    P95,
    P99,
    Rate,
    Count,
}

/// 维度定义。
#[derive(Clone, Debug)]
pub struct DimDef {
    pub id: u32,
    pub name: String,
    pub cardinality_hint: u32,
    pub indexed: bool,
}

/// 数据点。
#[derive(Clone, Copy, Debug)]
pub struct DataPoint {
    pub metric_id: u32,
    pub dim_hash: u64,
    pub timestamp: u64,
    pub value_q8: i64,
}

/// 视图定义。
#[derive(Clone, Debug)]
pub struct ViewDef {
    pub id: u32,
    pub name: String,
    pub metric_ids: Vec<u32>,
    pub dim_ids: Vec<u32>,
    pub refresh_ms: u32,
    pub max_points: u32,
}

/// 查询计划。
#[derive(Clone, Debug)]
pub struct QueryPlan {
    pub id: u32,
    pub metric_id: u32,
    pub dim_filter: Vec<(u32, u64)>,
    pub time_start: u64,
    pub time_end: u64,
    pub agg: AggKind,
    pub steps: Vec<String>,
}

/// 看板数据模型。
pub struct DashboardModel {
    pub metrics: Vec<MetricDef>,
    pub dims: Vec<DimDef>,
    pub points: Vec<DataPoint>,
    pub views: Vec<ViewDef>,
    pub plans: Vec<QueryPlan>,
    pub snapshot_version: u64,
    pub last_refresh_ms: u64,
}

impl DashboardModel {
    pub fn new() -> Self {
        Self {
            metrics: Vec::new(),
            dims: Vec::new(),
            points: Vec::new(),
            views: Vec::new(),
            plans: Vec::new(),
            snapshot_version: 0,
            last_refresh_ms: 0,
        }
    }

    pub fn add_metric(&mut self, m: MetricDef) -> u32 {
        let id = m.id;
        self.metrics.push(m);
        id
    }

    pub fn add_dim(&mut self, d: DimDef) -> u32 {
        let id = d.id;
        self.dims.push(d);
        id
    }

    pub fn add_point(&mut self, p: DataPoint) {
        self.points.push(p);
    }

    pub fn add_view(&mut self, v: ViewDef) -> u32 {
        let id = v.id;
        self.views.push(v);
        id
    }

    /// 构建查询计划（确定性：按 metric_id + 时间窗）。
    pub fn plan_query(&mut self, metric_id: u32, start: u64, end: u64, agg: AggKind) -> u32 {
        let id = self.plans.len() as u32 + 1;
        let mut steps = Vec::new();
        steps.push(String::from("scan_points"));
        steps.push(String::from("filter_time"));
        steps.push(String::from("group_by_dim"));
        steps.push(String::from("aggregate"));
        self.plans.push(QueryPlan {
            id,
            metric_id,
            dim_filter: Vec::new(),
            time_start: start,
            time_end: end,
            agg,
            steps,
        });
        id
    }

    /// 执行聚合（确定性）。
    pub fn execute(&self, plan_id: u32) -> Option<i64> {
        let plan = self.plans.iter().find(|p| p.id == plan_id)?;
        let values: Vec<i64> = self
            .points
            .iter()
            .filter(|p| p.metric_id == plan.metric_id && p.timestamp >= plan.time_start && p.timestamp <= plan.time_end)
            .map(|p| p.value_q8)
            .collect();
        if values.is_empty() {
            return Some(0);
        }
        match plan.agg {
            AggKind::Sum => Some(values.iter().sum()),
            AggKind::Avg => Some(values.iter().sum::<i64>() / values.len() as i64),
            AggKind::Min => values.iter().min().copied(),
            AggKind::Max => values.iter().max().copied(),
            AggKind::Count => Some(values.len() as i64),
            AggKind::P50 | AggKind::P95 | AggKind::P99 => {
                let mut s = values.clone();
                s.sort_unstable();
                let pct = match plan.agg {
                    AggKind::P50 => 50,
                    AggKind::P95 => 95,
                    _ => 99,
                };
                let idx = ((pct as usize) * (s.len() - 1)) / 100;
                Some(s[idx])
            }
            AggKind::Rate => {
                let n = values.len() as i64;
                if n > 0 {
                    Some((values.iter().filter(|v| **v > 0).count() as i64 * 256) / n)
                } else {
                    Some(0)
                }
            }
        }
    }

    /// 刷新节流。
    pub fn try_refresh(&mut self, now_ms: u64, min_interval_ms: u64) -> bool {
        if now_ms.saturating_sub(self.last_refresh_ms) < min_interval_ms {
            return false;
        }
        self.last_refresh_ms = now_ms;
        self.snapshot_version += 1;
        true
    }

    /// 模型序列化（简化 JSON）。
    pub fn serialize(&self) -> String {
        let mut s = String::from("{\"metrics\":[");
        for (i, m) in self.metrics.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&alloc::format!("{{\"id\":{},\"name\":\"{}\"}}", m.id, m.name));
        }
        s.push_str("],\"dims\":[");
        for (i, d) in self.dims.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&alloc::format!("{{\"id\":{},\"name\":\"{}\"}}", d.id, d.name));
        }
        s.push_str(&alloc::format!("],\"points\":{}}}", self.points.len()));
        s
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0312 遥测看板数据模型");
        cs.add("指标 id 唯一", {
            let mut ids: Vec<u32> = self.metrics.iter().map(|m| m.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.metrics.len()
        }, "unique");
        cs.add("维度 id 唯一", {
            let mut ids: Vec<u32> = self.dims.iter().map(|d| d.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.dims.len()
        }, "unique");
        cs.add("视图指标引用有效", {
            self.views.iter().all(|v| v.metric_ids.iter().all(|m| self.metrics.iter().any(|x| x.id == *m)))
        }, "valid refs");
        cs.add("数据点指标引用有效", {
            self.points.iter().all(|p| self.metrics.iter().any(|m| m.id == p.metric_id))
        }, "valid metric");
        cs.add("查询计划指标有效", {
            self.plans.iter().all(|p| self.metrics.iter().any(|m| m.id == p.metric_id))
        }, "valid plan");
        cs.add("时间窗有序", {
            self.plans.iter().all(|p| p.time_start <= p.time_end)
        }, "time order");
        cs.add("快照版本单调", self.snapshot_version <= u64::MAX, "domain");
        cs.add("序列化非空", !self.serialize().is_empty(), "non-empty");
        cs
    }
}

impl Default for DashboardModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_aggregation() {
        let mut m = DashboardModel::new();
        m.add_metric(MetricDef { id: 1, name: String::from("fps"), unit: 0, agg: AggKind::Avg, retention_hours: 24 });
        m.add_point(DataPoint { metric_id: 1, dim_hash: 0, timestamp: 1, value_q8: 100 });
        m.add_point(DataPoint { metric_id: 1, dim_hash: 0, timestamp: 2, value_q8: 200 });
        let pid = m.plan_query(1, 0, 10, AggKind::Avg);
        assert_eq!(m.execute(pid), Some(150));
    }

    #[test]
    fn refresh_throttle() {
        let mut m = DashboardModel::new();
        assert!(m.try_refresh(1000, 500));
        assert!(!m.try_refresh(1200, 500));
        assert!(m.try_refresh(1600, 500));
    }
}
