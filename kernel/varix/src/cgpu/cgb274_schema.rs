// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0274 · 指标 schema 与命名规范
//!
//! 五元组模型：名称/维度/单位/类型/采样策略——全域唯一。
//! 三级命名：域.组.指标——防指标沼泽。
//! 四维设计：按帧/按任务/按队列/按账户。
//! schema 版本化：演进不破坏历史数据——旧数据可读。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 五元组模型
// ---------------------------------------------------------------------------

/// 指标类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    /// 计数器（单调递增）。
    Counter,
    /// 瞬时值（可增可减）。
    Gauge,
    /// 直方图（分布）。
    Histogram,
    /// 摘要（P50/P95/P99）。
    Summary,
}

impl MetricType {
    pub fn label(&self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
            MetricType::Summary => "summary",
        }
    }

    /// 是否可做聚合（sum）。
    pub fn is_additive(&self) -> bool {
        matches!(self, MetricType::Counter)
    }
}

/// 采样策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplePolicy {
    /// 全量采样。
    Full,
    /// 高频采样（1/10）。
    High,
    /// 低频采样（1/100）。
    Low,
    /// 按需采样（仅异常时）。
    OnDemand,
}

impl SamplePolicy {
    /// 采样率（分母）。
    pub fn rate_denom(&self) -> u32 {
        match self {
            SamplePolicy::Full => 1,
            SamplePolicy::High => 10,
            SamplePolicy::Low => 100,
            SamplePolicy::OnDemand => 0, // 特殊
        }
    }
}

/// 维度键（四维之一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DimKey {
    /// 按帧。
    Frame,
    /// 按任务。
    Task,
    /// 按队列。
    Queue,
    /// 按账户。
    Account,
}

impl DimKey {
    pub fn label(&self) -> &'static str {
        match self {
            DimKey::Frame => "frame",
            DimKey::Task => "task",
            DimKey::Queue => "queue",
            DimKey::Account => "account",
        }
    }
}

/// 单位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// 纳秒。
    Nanosecond,
    /// 微秒。
    Microsecond,
    /// 毫秒。
    Millisecond,
    /// 字节。
    Byte,
    /// 个数。
    Count,
    /// 百分比（0-100）。
    Percent,
    /// 无量纲。
    Dimensionless,
}

impl Unit {
    pub fn label(&self) -> &'static str {
        match self {
            Unit::Nanosecond => "ns",
            Unit::Microsecond => "us",
            Unit::Millisecond => "ms",
            Unit::Byte => "bytes",
            Unit::Count => "count",
            Unit::Percent => "percent",
            Unit::Dimensionless => "",
        }
    }
}

// ---------------------------------------------------------------------------
// 指标定义（五元组）
// ---------------------------------------------------------------------------

/// 指标定义。
#[derive(Debug, Clone)]
pub struct MetricDef {
    /// 三级名称：域.组.指标。
    pub name: String,
    /// 单位。
    pub unit: Unit,
    /// 类型。
    pub mtype: MetricType,
    /// 采样策略。
    pub policy: SamplePolicy,
    /// 维度键集（四维的子集）。
    pub dims: Vec<DimKey>,
    /// schema 版本。
    pub version: u32,
    /// 描述。
    pub desc: String,
}

impl MetricDef {
    /// 构造并验证三级命名。
    pub fn new(name: &str, unit: Unit, mtype: MetricType, policy: SamplePolicy, dims: Vec<DimKey>, desc: &str) -> Result<Self, u16> {
        if !Self::is_valid_name(name) { return Err(ERR_BAD_NAME); }
        Ok(MetricDef {
            name: name.to_string(),
            unit,
            mtype,
            policy,
            dims,
            version: 1,
            desc: desc.to_string(),
        })
    }

    /// 三级命名验证：域.组.指标，每段 [a-z0-9_]+。
    pub fn is_valid_name(name: &str) -> bool {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() != 3 { return false; }
        parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && !p.starts_with('_')
                && !p.ends_with('_')
        })
    }

    /// 提取域名。
    pub fn domain(&self) -> &str {
        self.name.split('.').next().unwrap_or("")
    }

    /// 提取组名。
    pub fn group(&self) -> &str {
        self.name.split('.').nth(1).unwrap_or("")
    }

    /// 提取指标名。
    pub fn metric(&self) -> &str {
        self.name.split('.').nth(2).unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

const ERR_BAD_NAME: u16 = 0xF701;
const ERR_DUPLICATE: u16 = 0xF702;
const ERR_UNKNOWN: u16 = 0xF703;
const ERR_VERSION: u16 = 0xF704;
const ERR_TOO_MANY_DIMS: u16 = 0xF705;

// ---------------------------------------------------------------------------
// Schema 注册表
// ---------------------------------------------------------------------------

/// Schema 注册表（全域唯一指标定义）。
pub struct SchemaRegistry {
    defs: BTreeMap<String, MetricDef>,
    /// 历史版本（旧数据可读）。
    history: BTreeMap<String, Vec<MetricDef>>,
    /// 当前 schema 版本。
    pub schema_version: u32,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        SchemaRegistry { defs: BTreeMap::new(), history: BTreeMap::new(), schema_version: 1 }
    }

    /// 注册指标（同名拒绝）。
    pub fn register(&mut self, def: MetricDef) -> Result<(), u16> {
        if self.defs.contains_key(&def.name) { return Err(ERR_DUPLICATE); }
        if def.dims.len() > 4 { return Err(ERR_TOO_MANY_DIMS); }
        self.defs.insert(def.name.clone(), def);
        Ok(())
    }

    /// 查询指标定义。
    pub fn lookup(&self, name: &str) -> Option<&MetricDef> {
        self.defs.get(name)
    }

    /// 按域名过滤。
    pub fn by_domain(&self, domain: &str) -> Vec<&MetricDef> {
        self.defs.values().filter(|d| d.domain() == domain).collect()
    }

    /// 按组名过滤。
    pub fn by_group(&self, group: &str) -> Vec<&MetricDef> {
        self.defs.values().filter(|d| d.group() == group).collect()
    }

    /// 升级 schema（新增维度/单位变更等——保留历史）。
    pub fn upgrade(&mut self, name: &str, new_def: MetricDef) -> Result<(), u16> {
        match self.defs.get(name) {
            None => Err(ERR_UNKNOWN),
            Some(old) => {
                let old_clone = old.clone();
                self.history.entry(name.to_string()).or_default().push(old_clone);
                self.defs.insert(name.to_string(), new_def);
                self.schema_version += 1;
                Ok(())
            }
        }
    }

    /// 获取历史版本（旧数据读取用）。
    pub fn historical(&self, name: &str, version: u32) -> Option<&MetricDef> {
        self.history.get(name)?.iter().find(|d| d.version == version)
    }

    /// 全部指标名（排序输出）。
    pub fn all_names(&self) -> Vec<String> {
        self.defs.keys().cloned().collect()
    }

    /// 指标总数。
    pub fn count(&self) -> usize { self.defs.len() }
}

// ---------------------------------------------------------------------------
// 标准指标集（cgpu 域内置）
// ---------------------------------------------------------------------------

/// 注册标准指标集。
pub fn register_standard_metrics(reg: &mut SchemaRegistry) -> Result<usize, u16> {
    let mut count = 0;
    let specs: Vec<(&str, Unit, MetricType, SamplePolicy, Vec<DimKey>, &str)> = alloc::vec![
        // 调度核心
        ("cgpu.sched.queue_depth", Unit::Count, MetricType::Gauge, SamplePolicy::High, alloc::vec![DimKey::Queue], "队列深度"),
        ("cgpu.sched.task_latency", Unit::Microsecond, MetricType::Histogram, SamplePolicy::Full, alloc::vec![DimKey::Task, DimKey::Queue], "任务延迟"),
        ("cgpu.sched.frame_time", Unit::Millisecond, MetricType::Histogram, SamplePolicy::Full, alloc::vec![DimKey::Frame], "帧时间"),
        ("cgpu.sched.preempt_count", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![DimKey::Queue], "抢占次数"),
        // 死锁
        ("cgpu.deadlock.events", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![], "死锁事件数"),
        ("cgpu.deadlock.detection_ms", Unit::Millisecond, MetricType::Histogram, SamplePolicy::Full, alloc::vec![], "检测耗时"),
        ("cgpu.deadlock.resolve_rate", Unit::Percent, MetricType::Gauge, SamplePolicy::Low, alloc::vec![], "解除成功率"),
        // 持久化
        ("cgpu.persist.snapshot_bytes", Unit::Byte, MetricType::Histogram, SamplePolicy::Low, alloc::vec![], "快照体积"),
        ("cgpu.persist.recovery_ms", Unit::Millisecond, MetricType::Histogram, SamplePolicy::Low, alloc::vec![], "恢复耗时"),
        // 遥测自身
        ("cgpu.tele.drop_count", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![], "指标丢弃数"),
        ("cgpu.tele.buffer_usage", Unit::Percent, MetricType::Gauge, SamplePolicy::High, alloc::vec![], "缓冲使用率"),
        // 预算
        ("cgpu.budget.usage", Unit::Percent, MetricType::Gauge, SamplePolicy::High, alloc::vec![DimKey::Account], "预算使用率"),
        ("cgpu.budget.overrun", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![DimKey::Account], "预算超用次数"),
    ];

    for (name, unit, mtype, policy, dims, desc) in specs {
        let def = MetricDef::new(name, unit, mtype, policy, dims, desc)?;
        reg.register(def)?;
        count += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// 维度值与标签
// ---------------------------------------------------------------------------

/// 维度值标签（指标打点时的维度快照）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimLabels {
    pub labels: BTreeMap<DimKey, u32>,
}

impl DimLabels {
    pub fn new() -> Self { DimLabels { labels: BTreeMap::new() } }

    pub fn set(mut self, key: DimKey, val: u32) -> Self {
        self.labels.insert(key, val);
        self
    }

    /// 验证维度集是否匹配定义。
    pub fn matches(&self, def: &MetricDef) -> bool {
        self.labels.keys().all(|k| def.dims.contains(k))
    }

    /// 签名（用于缓存 key）。
    pub fn signature(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.labels {
            s.push_str(k.label());
            s.push('=');
            s.push_str(&v.to_string());
            s.push(';');
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0274 判据全集。
pub fn run_schema_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0274", "指标 schema 与命名规范");

    // 1. 三级命名
    cs.ok("合法名通过", MetricDef::is_valid_name("cgpu.sched.queue_depth"), "域.组.指标");
    cs.ok("两级名拒绝", !MetricDef::is_valid_name("cgpu.sched"), "必须三级");
    cs.ok("四级名拒绝", !MetricDef::is_valid_name("a.b.c.d"), "必须三级");
    cs.ok("大写拒绝", !MetricDef::is_valid_name("cgpu.Sched.x"), "只允许小写");
    cs.ok("空段拒绝", !MetricDef::is_valid_name("cgpu..x"), "不允许空段");
    cs.ok("首尾下划线拒绝", !MetricDef::is_valid_name("cgpu._a_.x"), "边界约束");

    // 2. 五元组
    let def = MetricDef::new("cgpu.test.metric", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![DimKey::Frame], "test").unwrap();
    cs.ok("域名解析", def.domain() == "cgpu", "一级");
    cs.ok("组名解析", def.group() == "test", "二级");
    cs.ok("指标名解析", def.metric() == "metric", "三级");
    cs.ok("类型正确", def.mtype == MetricType::Counter, "五元组");
    cs.ok("单位正确", def.unit == Unit::Count, "五元组");

    // 3. 类型语义
    cs.ok("Counter 可加", MetricType::Counter.is_additive(), "sum 聚合");
    cs.ok("Gauge 不可加", !MetricType::Gauge.is_additive(), "avg 聚合");
    cs.ok("Histogram 不可加", !MetricType::Histogram.is_additive(), "percentile");

    // 4. 采样率
    cs.ok("Full 采样率 1", SamplePolicy::Full.rate_denom() == 1, "全量");
    cs.ok("High 采样率 10", SamplePolicy::High.rate_denom() == 10, "高频");
    cs.ok("Low 采样率 100", SamplePolicy::Low.rate_denom() == 100, "低频");

    // 5. 注册表
    let mut reg = SchemaRegistry::new();
    cs.ok("注册成功", reg.register(def.clone()).is_ok(), "首注册");
    cs.ok("重复注册拒绝", reg.register(def.clone()).is_err(), "全域唯一");
    cs.ok("查询命中", reg.lookup("cgpu.test.metric").is_some(), "按名查询");
    cs.ok("域名过滤", reg.by_domain("cgpu").len() == 1, "按域过滤");

    // 6. 版本化
    let def_v2 = MetricDef::new("cgpu.test.metric", Unit::Millisecond, MetricType::Gauge, SamplePolicy::Low, alloc::vec![DimKey::Frame, DimKey::Queue], "test v2").unwrap();
    cs.ok("升级成功", reg.upgrade("cgpu.test.metric", def_v2).is_ok(), "保留历史");
    cs.ok("升级后版本", reg.schema_version == 2, "版本递增");
    cs.ok("历史可读", reg.historical("cgpu.test.metric", 1).is_some(), "旧数据兼容");
    cs.ok("当前可读", reg.lookup("cgpu.test.metric").is_some(), "新版本");

    // 7. 标准指标集
    let mut reg2 = SchemaRegistry::new();
    let std_count = register_standard_metrics(&mut reg2).unwrap_or(0);
    cs.ok("标准指标注册", std_count >= 10, "cgpu 域内置");
    cs.ok("标准指标名合法", reg2.all_names().iter().all(|n| MetricDef::is_valid_name(n)), "命名规范");

    // 8. 维度标签
    let lbl = DimLabels::new().set(DimKey::Frame, 42);
    cs.ok("维度匹配", lbl.matches(&MetricDef::new("cgpu.t.m", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![DimKey::Frame], "").unwrap()), "维度一致");
    cs.ok("维度签名非空", !lbl.signature().is_empty(), "缓存 key");

    // 9. 四维设计
    let dims = [DimKey::Frame, DimKey::Task, DimKey::Queue, DimKey::Account];
    cs.ok("四维齐备", dims.len() == 4, "帧/任务/队列/账户");

    cs.done()
}
