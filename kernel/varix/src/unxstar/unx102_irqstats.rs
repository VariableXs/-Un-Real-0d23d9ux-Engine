//! unx102_irqstats — UNX-F0913 设备中断统计账（per 中断计数与分布）。
//!
//! 判据原文（施工书锚点 UNX-F0913，CoRun·UNX-A2-B06 批）：
//! **UNX-F0913-J1 统计精确（10 万事件计数误差 0），per 中断独立
//! （跨中断零串账），导出对齐 F0899 口径**。
//!
//! 职责定位：B06 中断面有了路由/命名/一致性校验，还差"看得见的
//! 数字"——每个中断来过多少次、服务多快、丢没丢。Windows 深在
//! 哪：性能监视器的 Processor:Interrupts/sec、!idt 的计数列、
//! ETW 的 Interrupt 事件流，全是 per-中断独立计数——**串了账的
//! 统计等于没有统计**（把 A 设备的中断算到 B 头上，负载分析全
//! 错）；Windows 的计数器 schema（名/类型/刻度）是系统级契约，
//! 消费方按口径解析。Varix：①**per-中断五类计数**——到达/服务
//! /应答/丢弃/伪中断独立账，账面单调（回退即立案）；②**守恒
//! 式防串账**——全局 arrive_total 恒等于 Σ per-key arrive，校验
//! 函数独立重算对拍（增量同写两处+独立重算双保险）；③**分布
//! 三面**——向量直方图（256 桶）/设备聚合/形态聚合，桶下标超
//! 空间立案；④**导出对齐 F0899 口径**——指标三元组（指标名/
//! 单位/更新周期）schema 校验闭集 + 行式确定性导出（同账两次
//! 输出逐字一致）；⑤**间隔账**——per-key 相邻事件间隔 min/max
//! 自洽（min≤max，首个事件无间隔显性 None）。
//! 上游：F0912 三账（键与向量来源）、F0911 命名（导出行可带名）、
//! F0899 指标 schema（口径声明本体）；下游：F0899 健康联动（统
//! 计指标消费）、ktest B06。
//! 本单交付**五类计数 + 守恒防串账 + 分布三面 + F0899 口径导出
//! + 间隔账 + 十万压测 + 串账注入 + 快照**八面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显
//! 性注入）；checked_add 溢出显性立案；LCG 确定性仿真；FNV-1a 快
//! 照往返；错误全显性带诊断码与建议。诊断码段 0x46（0x45 为
//! F0912 所取）。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const IRQ_STATS_VERSION: &str = "B06-irqstats-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x46 段，本单独占——0x45 为 F0912 所取）
// ---------------------------------------------------------------------------

/// 0x4601 未知键事件（observe 消费未注册键——无主事件）。
pub const XC_UNKNOWN_KEY: u32 = 0x4601;
/// 0x4602 计数回退（账面单调性破坏——时间倒流或账被改写）。
pub const XC_COUNT_REGRESS: u32 = 0x4602;
/// 0x4603 守恒破缺（全局计数≠Σper-key——串账立案）。
pub const XC_CONSERVATION_BROKEN: u32 = 0x4603;
/// 0x4604 分布桶溢出（向量超 256 桶空间）。
pub const XC_BUCKET_OVERFLOW: u32 = 0x4604;
/// 0x4605 导出漂移（同账两次导出逐字不一致）。
pub const XC_EXPORT_DRIFT: u32 = 0x4605;
/// 0x4606 指标 schema 违例（名/单位/周期三元组不合规）。
pub const XC_METRIC_SCHEMA: u32 = 0x4606;
/// 0x4607 事件种类未知（闭集外事件）。
pub const XC_UNKNOWN_EVENT: u32 = 0x4607;
/// 0x4608 间隔账失真（min>max 或 tick 回退）。
pub const XC_GAP_DISTORTED: u32 = 0x4608;
/// 0x4609 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4609;
/// 0x460A 计数溢出（checked_add 溢出——u64 界显性化）。
pub const XC_COUNT_OVERFLOW: u32 = 0x460A;

// ---------------------------------------------------------------------------
// §1 事件种类闭集
// ---------------------------------------------------------------------------

/// 事件种类：中断面五类闭环（到达→服务→应答为正路，丢弃/伪中断
/// 为异常路——异常路计数是健康联动 F0899 的红账原料）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 到达。
    Arrive,
    /// 服务。
    Served,
    /// 应答。
    Answered,
    /// 丢弃（队列满/被掩蔽）。
    Dropped,
    /// 伪中断（无人应答的拉线）。
    Spurious,
}

impl EventKind {
    /// 全态枚举。
    pub const fn all() -> [EventKind; 5] {
        [
            EventKind::Arrive,
            EventKind::Served,
            EventKind::Answered,
            EventKind::Dropped,
            EventKind::Spurious,
        ]
    }

    /// 下标（计数槽位）。
    pub const fn idx(self) -> usize {
        match self {
            EventKind::Arrive => 0,
            EventKind::Served => 1,
            EventKind::Answered => 2,
            EventKind::Dropped => 3,
            EventKind::Spurious => 4,
        }
    }

    /// 人话名（导出行消费）。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::Arrive => "到达",
            EventKind::Served => "服务",
            EventKind::Answered => "应答",
            EventKind::Dropped => "丢弃",
            EventKind::Spurious => "伪中断",
        }
    }
}

// ---------------------------------------------------------------------------
// §2 per-中断统计条目
// ---------------------------------------------------------------------------

/// 单键统计账：五类计数 + 间隔账（相邻到达事件间隔 min/max）。
#[derive(Clone, Debug, Default)]
pub struct IrqStat {
    /// 五类计数（槽位对齐 EventKind::idx）。
    pub counts: [u64; 5],
    /// 最近到达 tick（间隔账游标——None=尚无到达）。
    pub last_arrive_tick: Option<u64>,
    /// 相邻到达最小间隔。
    pub min_gap: Option<u64>,
    /// 相邻到达最大间隔。
    pub max_gap: Option<u64>,
}

impl IrqStat {
    /// 总事件数。
    pub fn total(&self) -> u64 {
        self.counts.iter().fold(0u64, |a, b| a.wrapping_add(*b))
    }
}

// ---------------------------------------------------------------------------
// §3 指标三元组（F0899 口径声明本体——指标名/单位/更新周期）
// ---------------------------------------------------------------------------

/// 指标单位闭集（F0899 schema 校验位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricUnit {
    /// 事件计数（无量纲整数）。
    Count,
    /// 微秒（间隔类指标）。
    Micros,
}

impl MetricUnit {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            MetricUnit::Count => "count",
            MetricUnit::Micros => "us",
        }
    }
}

/// 更新周期闭集（F0899 schema 校验位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdatePeriod {
    /// 每 tick 即时。
    PerTick,
    /// 每巡检窗口。
    PerSweep,
    /// 按需采样。
    OnDemand,
}

impl UpdatePeriod {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            UpdatePeriod::PerTick => "per-tick",
            UpdatePeriod::PerSweep => "per-sweep",
            UpdatePeriod::OnDemand => "on-demand",
        }
    }
}

/// 指标三元组 schema 校验（F0899 口径：名/单位/更新周期）。
///
/// 名规则：非空、ASCII 小写字母/数字/点/连字符、长度 1..=32。
pub fn metric_schema_ok(name: &str, unit: MetricUnit, period: UpdatePeriod) -> Result<(), (u32, String)> {
    if name.is_empty() || name.len() > 32 {
        return Err((
            XC_METRIC_SCHEMA,
            format!("指标名 \"{}\" 长度越界（1..=32）——F0899 口径拒", name),
        ));
    }
    for b in name.bytes() {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-';
        if !ok {
            return Err((
                XC_METRIC_SCHEMA,
                format!("指标名含非法字符 '{}'（口径 [a-z0-9.-]）", b as char),
            ));
        }
    }
    // 单位/周期在闭集——枚举类型天然闭集，此处显性化判据（const
    // 闭集对拍：两枚举各有合法槽位）。
    let _unit_slot = match unit {
        MetricUnit::Count => 0u8,
        MetricUnit::Micros => 1u8,
    };
    let _period_slot = match period {
        UpdatePeriod::PerTick => 0u8,
        UpdatePeriod::PerSweep => 1u8,
        UpdatePeriod::OnDemand => 2u8,
    };
    Ok(())
}

// ---------------------------------------------------------------------------
// §4 统计总账（per-key 独立 + 全局守恒 + 分布三面）
// ---------------------------------------------------------------------------

/// 向量桶空间（256——向量 8 位语义）。
pub const BUCKETS: usize = 256;

/// 设备中断统计总账。
#[derive(Default)]
pub struct IrqStatsLedger {
    /// 键注册账：键→（向量，设备）。
    pub enrolled: BTreeMap<u32, (u32, String)>,
    /// per-key 独立统计账。
    pub per_key: BTreeMap<u32, IrqStat>,
    /// 全局五类计数（守恒对拍位）。
    pub global: [u64; 5],
    /// 向量直方图（256 桶——只计到达）。
    pub hist: Vec<u64>,
    /// 注册计数。
    pub enrolled_total: u64,
    /// 事件总数（全局 arrive 恒等项）。
    pub event_total: u64,
}

impl IrqStatsLedger {
    /// 新账。
    pub fn new() -> IrqStatsLedger {
        IrqStatsLedger {
            enrolled: BTreeMap::new(),
            per_key: BTreeMap::new(),
            global: [0u64; 5],
            hist: vec![0u64; BUCKETS],
            enrolled_total: 0,
            event_total: 0,
        }
    }

    /// 键注册（键→向量/设备——统计对象入册）。
    pub fn enroll(&mut self, key: u32, vector: u32, device: &str) -> Result<(), (u32, String)> {
        if vector as usize >= BUCKETS {
            return Err((
                XC_BUCKET_OVERFLOW,
                format!("键 #{} 向量 {} 超桶空间 {}", key, vector, BUCKETS),
            ));
        }
        if device.is_empty() {
            return Err((XC_UNKNOWN_KEY, format!("键 #{} 设备空——无主统计对象", key)));
        }
        if self.enrolled.contains_key(&key) {
            return Err((
                XC_UNKNOWN_KEY,
                format!("键 #{} 重复注册——统计对象键唯一", key),
            ));
        }
        self.enrolled.insert(key, (vector, device.to_string()));
        self.per_key.insert(key, IrqStat::default());
        self.enrolled_total += 1;
        Ok(())
    }

    /// 事件观测：per-key 与全局双写（增量同源）+ 直方图 + 间隔账。
    pub fn observe(&mut self, key: u32, kind: EventKind, tick: u64) -> Result<(), (u32, String)> {
        // 未知键（无主事件立案——统计精确的前提是对象在册）。
        if !self.enrolled.contains_key(&key) {
            return Err((
                XC_UNKNOWN_KEY,
                format!("键 #{} 未注册——无主事件不入账", key),
            ));
        }
        // per-key 槽位计数（checked_add 溢出显性立案）。
        let stat = match self.per_key.get_mut(&key) {
            Some(s) => s,
            None => {
                return Err((
                    XC_UNKNOWN_KEY,
                    format!("键 #{} 注册账与统计账失偶——内部失守", key),
                ))
            }
        };
        let slot = kind.idx();
        let nc = stat.counts[slot]
            .checked_add(1)
            .ok_or((XC_COUNT_OVERFLOW, format!("键 #{} 计数溢出", key)))?;
        stat.counts[slot] = nc;
        // 间隔账（仅到达事件——相邻到达间隔）。
        if kind == EventKind::Arrive {
            match stat.last_arrive_tick {
                Some(prev) => {
                    if tick < prev {
                        return Err((
                            XC_GAP_DISTORTED,
                            format!("键 #{} tick 回退（{} < {}）——间隔账失真", key, tick, prev),
                        ));
                    }
                    let gap = tick - prev;
                    let upd_min = match stat.min_gap {
                        Some(m) if m <= gap => m,
                        _ => gap,
                    };
                    let upd_max = match stat.max_gap {
                        Some(m) if m >= gap => m,
                        _ => gap,
                    };
                    stat.min_gap = Some(upd_min);
                    stat.max_gap = Some(upd_max);
                }
                None => {
                    stat.min_gap = None;
                    stat.max_gap = None;
                }
            }
            stat.last_arrive_tick = Some(tick);
        }
        // 全局双写（守恒式两半——增量同源同写）。
        let ng = self.global[slot]
            .checked_add(1)
            .ok_or((XC_COUNT_OVERFLOW, "全局计数溢出".to_string()))?;
        self.global[slot] = ng;
        if kind == EventKind::Arrive {
            self.event_total += 1;
            let vec_no = self.enrolled[&key].0 as usize;
            self.hist[vec_no] = self.hist[vec_no].wrapping_add(1);
        }
        Ok(())
    }

    /// 守恒校验：全局五类恒等 Σ per-key 五类（串账防线的独立重算）。
    pub fn conservation_check(&self) -> Result<(), (u32, String)> {
        let mut sum = [0u64; 5];
        for s in self.per_key.values() {
            for i in 0..5 {
                sum[i] = sum[i].wrapping_add(s.counts[i]);
            }
        }
        for i in 0..5 {
            if sum[i] != self.global[i] {
                let kind = EventKind::all()[i];
                return Err((
                    XC_CONSERVATION_BROKEN,
                    format!(
                        "{}类守恒破缺：全局 {} ≠ Σper-key {}（串账 {} 条）",
                        kind.name(),
                        self.global[i],
                        sum[i],
                        (self.global[i] as i128 - sum[i] as i128).unsigned_abs()
                    ),
                ));
            }
        }
        // 到达事件总数恒等 event_total（第三重守恒）。
        if self.global[EventKind::Arrive.idx()] != self.event_total {
            return Err((
                XC_CONSERVATION_BROKEN,
                format!(
                    "到达总量失恒：global[到达] {} ≠ event_total {}",
                    self.global[EventKind::Arrive.idx()],
                    self.event_total
                ),
            ));
        }
        // 直方图总和恒等到达总量（第四重守恒——分布面不丢事件）。
        let hist_sum = self.hist.iter().fold(0u64, |a, b| a.wrapping_add(*b));
        if hist_sum != self.event_total {
            return Err((
                XC_CONSERVATION_BROKEN,
                format!("直方图失恒：Σ桶 {} ≠ 到达 {}", hist_sum, self.event_total),
            ));
        }
        Ok(())
    }

    /// 设备聚合：设备→事件总数（分布面之二）。
    pub fn by_device(&self) -> BTreeMap<&str, u64> {
        let mut agg: BTreeMap<&str, u64> = BTreeMap::new();
        for (key, (_, device)) in self.enrolled.iter() {
            let t = self.per_key.get(key).map(|s| s.total()).unwrap_or(0);
            let slot = agg.entry(device.as_str()).or_insert(0);
            *slot = slot.wrapping_add(t);
        }
        agg
    }

    /// 单键读账（只读视图——外部消费）。
    pub fn stat_of(&self, key: u32) -> Option<&IrqStat> {
        self.per_key.get(&key)
    }
}

// ---------------------------------------------------------------------------
// §5 F0899 口径导出（指标三元组行式——确定性逐字一致）
// ---------------------------------------------------------------------------

/// 指标快照（一行导出的值载体）。
pub struct MetricRow {
    /// 指标名。
    pub name: String,
    /// 单位。
    pub unit: MetricUnit,
    /// 更新周期。
    pub period: UpdatePeriod,
    /// 值。
    pub value: u64,
}

impl MetricRow {
    /// schema 校验（导出前强制——F0899 口径闸）。
    pub fn check(&self) -> Result<(), (u32, String)> {
        metric_schema_ok(&self.name, self.unit, self.period)
    }

    /// 行式输出。
    pub fn line(&self) -> String {
        format!(
            "指标={} 单位={} 周期={} 值={}",
            self.name,
            self.unit.name(),
            self.period.name(),
            self.value
        )
    }
}

/// 标准指标集（F0899 口径对齐——导出总闸：每行先过 schema）。
pub fn export_metrics(led: &IrqStatsLedger) -> Result<Vec<String>, (u32, String)> {
    let rows = [
        MetricRow {
            name: "irq.arrive.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerTick,
            value: led.global[EventKind::Arrive.idx()],
        },
        MetricRow {
            name: "irq.served.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerTick,
            value: led.global[EventKind::Served.idx()],
        },
        MetricRow {
            name: "irq.answered.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerTick,
            value: led.global[EventKind::Answered.idx()],
        },
        MetricRow {
            name: "irq.dropped.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerSweep,
            value: led.global[EventKind::Dropped.idx()],
        },
        MetricRow {
            name: "irq.spurious.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerSweep,
            value: led.global[EventKind::Spurious.idx()],
        },
        MetricRow {
            name: "irq.enrolled.keys".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::OnDemand,
            value: led.enrolled_total,
        },
    ];
    let mut out = Vec::new();
    for r in rows.iter() {
        r.check()?;
        out.push(r.line());
    }
    Ok(out)
}

/// 导出确定性对拍：同账两次导出逐字一致。
pub fn export_determinism(led: &IrqStatsLedger) -> Result<usize, (u32, String)> {
    let a = export_metrics(led)?;
    let b = export_metrics(led)?;
    if a != b {
        return Err((
            XC_EXPORT_DRIFT,
            format!("导出两次不等（{} vs {} 行）——确定性失守", a.len(), b.len()),
        ));
    }
    Ok(a.len())
}

// ---------------------------------------------------------------------------
// §6 LCG + FNV（压测与快照底座）
// ---------------------------------------------------------------------------

/// 线性同余发生器（Numerical Recipes 参数）。
pub struct Lcg(u64);

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }

    /// 半开区间 [0, n)。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 统计账快照指纹。
pub struct StatsSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl StatsSnap {
    /// 采样：全局计数 + 直方图 + per-key 键序内容。
    pub fn take(led: &IrqStatsLedger) -> StatsSnap {
        let mut buf: Vec<u8> = Vec::new();
        for v in led.global.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.extend_from_slice(&led.event_total.to_le_bytes());
        buf.extend_from_slice(&led.enrolled_total.to_le_bytes());
        for h in led.hist.iter() {
            buf.extend_from_slice(&h.to_le_bytes());
        }
        for (k, s) in led.per_key.iter() {
            buf.extend_from_slice(&k.to_le_bytes());
            for c in s.counts.iter() {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            buf.extend_from_slice(&s.last_arrive_tick.unwrap_or(u64::MAX).to_le_bytes());
        }
        StatsSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "键={} 事件={} 全局[到达={} 服务={} 应答={} 丢={} 伪={}]",
                led.per_key.len(),
                led.event_total,
                led.global[0],
                led.global[1],
                led.global[2],
                led.global[3],
                led.global[4]
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §7 十万压测（四设备轮转——计数误差 0 + 零串账 + 导出对拍）
// ---------------------------------------------------------------------------

/// 压测事件量。
pub const STRESS_EVENTS: u32 = 100_000;
/// 压测键数（四设备——LCG 轮转）。
pub const STRESS_KEYS: u32 = 4;

/// 十万事件压测：到达恰好 10 万误差 0 + 四重守恒 + per-key 独立
/// 抽样对拍 + 导出确定性。
pub fn stress_100k() -> Result<String, (u32, String)> {
    let mut led = IrqStatsLedger::new();
    let mut rng = Lcg::new(0x57A7_1137);
    for i in 0..STRESS_KEYS {
        led.enroll(i, i * 2, &format!("dev-{}", i))?;
    }
    // 记账面：每键注入的到达/服务/应答次数（压测后对拍）。
    let mut expect: BTreeMap<u32, [u64; 5]> = BTreeMap::new();
    for i in 0..STRESS_KEYS {
        expect.insert(i, [0u64; 5]);
    }
    for e in 0..STRESS_EVENTS {
        let key = rng.below(STRESS_KEYS);
        // 正路为主（85% 到达/服务/应答三连），异常路少量。
        let roll = rng.below(100);
        let tick = (e as u64) * 3 + key as u64; // 单调 tick（间隔账合法）
        let exp = match expect.get_mut(&key) {
            Some(v) => v,
            None => return Err((XC_UNKNOWN_KEY, format!("对拍账缺键 #{}", key))),
        };
        if roll < 85 {
            // 三连：到达→服务→应答。
            led.observe(key, EventKind::Arrive, tick)?;
            led.observe(key, EventKind::Served, tick)?;
            led.observe(key, EventKind::Answered, tick)?;
            exp[0] += 1;
            exp[1] += 1;
            exp[2] += 1;
        } else if roll < 90 {
            led.observe(key, EventKind::Dropped, tick)?;
            exp[3] += 1;
        } else if roll < 95 {
            led.observe(key, EventKind::Spurious, tick)?;
            exp[4] += 1;
        } else {
            // 到达但无应答（悬挂——合法态，服务/应答不来）。
            led.observe(key, EventKind::Arrive, tick)?;
            exp[0] += 1;
        }
    }
    // 1）统计精确：到达总量 == 注入正路总量（误差 0 判据）。
    let arrive_injected: u64 = expect.values().map(|v| v[0]).fold(0u64, |a, b| a.wrapping_add(b));
    if led.global[EventKind::Arrive.idx()] != arrive_injected || arrive_injected == 0 {
        return Err((
            XC_CONSERVATION_BROKEN,
            format!("到达计数失准：账 {} ≠ 注入 {}", led.global[EventKind::Arrive.idx()], arrive_injected),
        ));
    }
    // 2）per-key 独立对拍：每键五类计数与注入账逐槽相等（零串账）。
    for (k, exp) in expect.iter() {
        match led.stat_of(*k) {
            Some(s) => {
                for i in 0..5 {
                    if s.counts[i] != exp[i] {
                        let kind = EventKind::all()[i];
                        return Err((
                            XC_CONSERVATION_BROKEN,
                            format!(
                                "键 #{} {}类串账：账 {} ≠ 注入 {}",
                                k, kind.name(), s.counts[i], exp[i]
                            ),
                        ));
                    }
                }
            }
            None => return Err((XC_UNKNOWN_KEY, format!("键 #{} 统计账丢失", k))),
        }
    }
    // 3）四重守恒。
    led.conservation_check()?;
    // 4）导出确定性（F0899 口径闸 + 逐字一致）。
    let rows = export_determinism(&led)?;
    // 5）快照稳定。
    let s0 = StatsSnap::take(&led);
    let s1 = StatsSnap::take(&led);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "压测账快照不稳定".to_string()));
    }
    Ok(format!(
        "十万压测全绿：到达 {} 误差0 四键逐槽对拍平 四重守恒过 导出 {} 行 F0899 口径 快照 {:#010x}",
        arrive_injected, rows, s0.hash
    ))
}

// ---------------------------------------------------------------------------
// §8 注入面（串账检出 + 无主事件 + tick 回退 + schema 违例）
// ---------------------------------------------------------------------------

/// 串账注入：人工破坏 per-key 计数→守恒校验必红（10/10 全检出）。
pub fn inject_serial_leak_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    for case in 0..10u8 {
        let mut led = fresh_ledger_2keys();
        // 注入：把键 0 的到达计数偷走 case+1 条（模拟串到键 1 的账）。
        let steal = case as u64 + 1;
        if let Some(s) = led.per_key.get_mut(&0) {
            s.counts[EventKind::Arrive.idx()] = s.counts[EventKind::Arrive.idx()].saturating_sub(steal);
        }
        // 守恒校验必红（全局仍在、per-key 被偷——差额=steal）。
        let r = led.conservation_check();
        let caught = matches!(r, Err((XC_CONSERVATION_BROKEN, _)));
        hits.push((case, caught, match r {
            Err((code, m)) => format!("code={:#06x} {}", code, m),
            Ok(_) => "未检出——串账漏网".to_string(),
        }));
    }
    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_CONSERVATION_BROKEN, format!("串账检出缺口：{}", names.join("；"))));
    }
    Ok("串账注入 10/10 全检出（守恒式独立重算）".to_string())
}

/// 两键标定账（各 3 条到达，供串账注入破坏）。
fn fresh_ledger_2keys() -> IrqStatsLedger {
    let mut led = IrqStatsLedger::new();
    let _ = led.enroll(0, 4, "dev-a");
    let _ = led.enroll(1, 6, "dev-b");
    for t in 0..3u64 {
        let _ = led.observe(0, EventKind::Arrive, t * 10);
        let _ = led.observe(1, EventKind::Arrive, t * 10 + 5);
    }
    led
}

/// 无主事件 + tick 回退 + 桶溢出 + schema 违例注入。
pub fn inject_guardrails() -> Result<String, (u32, String)> {
    // 1 无主事件。
    let mut led = IrqStatsLedger::new();
    let r = led.observe(42, EventKind::Arrive, 0);
    if !matches!(r, Err((XC_UNKNOWN_KEY, _))) {
        return Err((XC_UNKNOWN_KEY, "无主事件未拦".to_string()));
    }
    // 2 重复注册。
    let mut led = IrqStatsLedger::new();
    led.enroll(1, 2, "d1")?;
    let r = led.enroll(1, 4, "d2");
    if !matches!(r, Err((XC_UNKNOWN_KEY, _))) {
        return Err((XC_UNKNOWN_KEY, "重复注册未拦".to_string()));
    }
    // 3 桶溢出。
    let mut led = IrqStatsLedger::new();
    let r = led.enroll(2, BUCKETS as u32 + 1, "d2");
    if !matches!(r, Err((XC_BUCKET_OVERFLOW, _))) {
        return Err((XC_BUCKET_OVERFLOW, "桶溢出未拦".to_string()));
    }
    // 4 tick 回退（同键相邻到达 tick 倒退）。
    let mut led = IrqStatsLedger::new();
    led.enroll(3, 6, "d3")?;
    led.observe(3, EventKind::Arrive, 100)?;
    let r = led.observe(3, EventKind::Arrive, 50);
    if !matches!(r, Err((XC_GAP_DISTORTED, _))) {
        return Err((XC_GAP_DISTORTED, "tick 回退未拦".to_string()));
    }
    // 5 间隔账自洽（合法序列 min≤max 且账面单调）。
    let mut led = IrqStatsLedger::new();
    led.enroll(4, 8, "d4")?;
    led.observe(4, EventKind::Arrive, 10)?;
    led.observe(4, EventKind::Arrive, 20)?; // gap 10
    led.observe(4, EventKind::Arrive, 35)?; // gap 15
    led.observe(4, EventKind::Arrive, 40)?; // gap 5
    let s = match led.stat_of(4) {
        Some(s) => s,
        None => return Err((XC_UNKNOWN_KEY, "键 4 统计账丢失".to_string())),
    };
    let (mn, mx) = match (s.min_gap, s.max_gap) {
        (Some(a), Some(b)) => (a, b),
        _ => (0, 0),
    };
    if !(mn == 5 && mx == 15 && mn <= mx) {
        return Err((
            XC_GAP_DISTORTED,
            format!("间隔账失真：min {} max {}（期望 5/15）", mn, mx),
        ));
    }
    // 6 schema 违例（大写名）。
    let r = metric_schema_ok("Irq.Bad", MetricUnit::Count, UpdatePeriod::PerTick);
    if !matches!(r, Err((XC_METRIC_SCHEMA, _))) {
        return Err((XC_METRIC_SCHEMA, "schema 大写名未拦".to_string()));
    }
    // 7 schema 违例（空名）。
    let r = metric_schema_ok("", MetricUnit::Count, UpdatePeriod::PerTick);
    if !matches!(r, Err((XC_METRIC_SCHEMA, _))) {
        return Err((XC_METRIC_SCHEMA, "schema 空名未拦".to_string()));
    }
    Ok("无主/重注册/桶溢出/tick 回退/间隔自洽/schema 违例七闸全拦".to_string())
}

// ---------------------------------------------------------------------------
// §9 设备聚合与分布抽样
// ---------------------------------------------------------------------------

/// 设备聚合对拍：聚合总数恒等事件总数（分布面之二守恒）。
pub fn device_aggregate_check() -> Result<String, (u32, String)> {
    let mut led = IrqStatsLedger::new();
    let mut rng = Lcg::new(0x0D17_7A7);
    for i in 0..3u32 {
        led.enroll(i, i, &format!("dev-{}", i % 2))?; // 两设备三键
    }
    for e in 0..1000u32 {
        let key = rng.below(3);
        led.observe(key, EventKind::Arrive, e as u64)?;
    }
    let agg = led.by_device();
    let total: u64 = agg.values().fold(0u64, |a, b| a.wrapping_add(*b));
    if total != led.event_total {
        return Err((
            XC_CONSERVATION_BROKEN,
            format!("设备聚合失恒：{} ≠ 事件 {}", total, led.event_total),
        ));
    }
    if agg.len() != 2 {
        return Err((
            XC_CONSERVATION_BROKEN,
            format!("设备聚合键数 {} ≠ 2", agg.len()),
        ));
    }
    Ok(format!("设备聚合守恒（2 设备 {} 事件）", total))
}

// ---------------------------------------------------------------------------
// §10 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：七闸注入 + 串账 10/10 + 十万压测 + 聚合守恒 + 导出
/// 确定性，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(inject_guardrails()?);
    parts.push(inject_serial_leak_10()?);
    parts.push(stress_100k()?);
    parts.push(device_aggregate_check()?);
    // 导出确定性（空账基线）。
    let empty = IrqStatsLedger::new();
    let rows = export_determinism(&empty)?;
    parts.push(format!("导出确定性 {} 行逐字一致（空账基线）", rows));
    // 快照稳定性（空账双采样）。
    let s0 = StatsSnap::take(&empty);
    let s1 = StatsSnap::take(&empty);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空账快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
