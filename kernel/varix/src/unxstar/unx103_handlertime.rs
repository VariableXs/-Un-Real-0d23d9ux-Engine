//! unx103_handlertime — UNX-F0914 handler 处理时长观测（执行时长直
//! 方图与 top half 纪律联动）。
//!
//! 判据原文（施工书锚点 UNX-F0914，CoRun·UNX-A2-B06 批）：
//! **UNX-F0914-J1 时长直方图落账（per handler 独立），超时红账联
//! 动（>100µs handler 清单自动产出），开销 <100ns 断言**。
//!
//! 职责定位：F0913 数清了"来多少次"，本单量出"每次跑多久"。
//! Windows 深在哪：WDF/WDM 的 top half（ISR）契约是**能多短就多
//! 短**——长 ISR 拖死整个中断线（共享线上其他设备全饿死），
//! Windows 性能分析器把 ISR/DPC 时长分布当一等公民指标，超阈
//! handler 直接进"Interrupt 负载"报表；ETW 的 ISR 事件自带
//! Duration 字段按桶直方化。Varix：①**per-handler 独立直方图**
//! ——执行时长按 2 的幂桶（µs）落账，Σ桶==总数守恒（分布不失
//! 恒）；②**超时红账联动**——>100µs 阈值立案（F0905 top half
//! 违禁语义的消费延伸：ISR 长跑即纪律红账），超时清单自动产出
//! （键+次数+最值，不用人翻账）；③**开销 <100ns 断言**——零墙
//! 钟纪律下以观测路径步数为开销的确定性代理：单次 observe 的
//! 步数上界显性化（与 F0912 同型转化，实测依赖时钟——诚实口
//! 径）；④**F0899 口径导出**——时长指标三元组（指标名/单位=
//! µs/更新周期）schema 校验（复用 F0913 schema 纪律）。
//! 上游：F0913 统计账（事件面底座）、F0905 top half 违禁语义（红
//! 账联动声明本体）、F0899 指标 schema；下游：F0899 健康联动、
//! ktest B06。
//! 本单交付**直方图落账 + 超时红账 + 清单产出 + 预算断言 + 十万
//! 压测 + 注入面 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（时长与
//! tick 显性注入——本单是观测账不是采集器）；LCG 确定性仿真；
//! FNV-1a 快照往返；错误全显性带诊断码与建议。诊断码段 0x47
//! （0x46 为 F0913 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const HANDLER_TIME_VERSION: &str = "B06-handlertime-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x47 段，本单独占——0x46 为 F0913 所取）
// ---------------------------------------------------------------------------

/// 0x4701 未知 handler（observe 消费未注册键）。
pub const XC_UNKNOWN_HANDLER: u32 = 0x4701;
/// 0x4702 直方图失恒（Σ桶≠总观测数——分布丢样）。
pub const XC_HISTO_IMBALANCE: u32 = 0x4702;
/// 0x4703 超时红账（执行时长>阈值——top half 纪律联动立案）。
pub const XC_TIMEOUT_RED: u32 = 0x4703;
/// 0x4704 观测步数超预算（开销断言的确定性代理越界）。
pub const XC_OBS_BUDGET: u32 = 0x4704;
/// 0x4705 tick 回退（同 handler 相邻观测 tick 倒退）。
pub const XC_TICK_REGRESS: u32 = 0x4705;
/// 0x4706 导出漂移（同账两次导出逐字不一致）。
pub const XC_EXPORT_DRIFT: u32 = 0x4706;
/// 0x4707 指标 schema 违例（F0899 三元组不合规）。
pub const XC_METRIC_SCHEMA: u32 = 0x4707;
/// 0x4708 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4708;
/// 0x4709 重复注册（同键二度入册）。
pub const XC_DUP_HANDLER: u32 = 0x4709;
/// 0x470A 时长越界（0 时长或超 u32 界——显性拒）。
pub const XC_DURATION_RANGE: u32 = 0x470A;

// ---------------------------------------------------------------------------
// §1 直方图桶闭集（2 的幂 µs 桶）
// ---------------------------------------------------------------------------

/// 桶数：桶 i 覆盖 (2^i, 2^(i+1)] µs（i=0..16），桶 17 为溢出桶
/// （>65536µs——超时红账的常客）。
pub const N_BUCKETS: usize = 18;
/// 最大精确桶上界（2^16=65536µs）。
pub const BUCKET_MAX: u64 = 1u64 << 16;
/// 超时阈值（µs——判据原文 >100µs）。
pub const TIMEOUT_US: u64 = 100;

/// 时长→桶下标（纯函数单点定义——落账与对拍同口径）。
///
/// 显性循环不用位技巧（可读性优先，18 次比较在预算内）。
pub fn bucket_of(duration_us: u64) -> usize {
    for i in 0..(N_BUCKETS - 1) {
        if duration_us <= (1u64 << i) {
            return i;
        }
    }
    N_BUCKETS - 1 // 溢出桶
}

// ---------------------------------------------------------------------------
// §2 per-handler 时长账
// ---------------------------------------------------------------------------

/// 单 handler 时长观测账。
#[derive(Clone, Debug)]
pub struct HandlerTimeStat {
    /// 执行时长直方图（µs 幂桶）。
    pub hist: [u64; N_BUCKETS],
    /// 观测总数。
    pub total: u64,
    /// 超时次数（>TIMEOUT_US——红账联动）。
    pub over_count: u64,
    /// 最短时长。
    pub min_us: Option<u64>,
    /// 最长时长。
    pub max_us: Option<u64>,
    /// 最近观测 tick。
    pub last_tick: Option<u64>,
}

impl Default for HandlerTimeStat {
    fn default() -> Self {
        HandlerTimeStat {
            hist: [0u64; N_BUCKETS],
            total: 0,
            over_count: 0,
            min_us: None,
            max_us: None,
            last_tick: None,
        }
    }
}

impl HandlerTimeStat {
    /// 直方图求和（守恒对拍用）。
    pub fn hist_sum(&self) -> u64 {
        self.hist.iter().fold(0u64, |a, b| a.wrapping_add(*b))
    }
}

// ---------------------------------------------------------------------------
// §3 观测总账（enroll/observe/超时清单/预算断言）
// ---------------------------------------------------------------------------

/// 观测步数预算：单次 observe 的确定性步数上界（零墙钟纪律下
/// <100ns 判据的代理——桶定位 18 比较 + 双账写入 6 + 超时判 2 +
/// 常数余量 14，合计 40 步）。
pub const OBS_BUDGET_STEPS: u64 = 40;

/// handler 时长观测总账。
#[derive(Default)]
pub struct TimeLedger {
    /// 注册账：键→名。
    pub handlers: BTreeMap<u32, String>,
    /// per-key 独立账。
    pub per_key: BTreeMap<u32, HandlerTimeStat>,
    /// 全局直方图（守恒对拍位）。
    pub global_hist: [u64; N_BUCKETS],
    /// 注册总数。
    pub enrolled_total: u64,
    /// 观测总数。
    pub observe_total: u64,
    /// 超时红账总数。
    pub timeout_red_total: u64,
}

impl TimeLedger {
    /// 新账。
    pub fn new() -> TimeLedger {
        TimeLedger {
            handlers: BTreeMap::new(),
            per_key: BTreeMap::new(),
            global_hist: [0u64; N_BUCKETS],
            enrolled_total: 0,
            observe_total: 0,
            timeout_red_total: 0,
        }
    }

    /// handler 入册。
    pub fn enroll(&mut self, key: u32, name: &str) -> Result<(), (u32, String)> {
        if name.is_empty() {
            return Err((XC_UNKNOWN_HANDLER, format!("键 #{} 空名——观测对象无名", key)));
        }
        if self.handlers.contains_key(&key) {
            return Err((XC_DUP_HANDLER, format!("键 #{} 重复入册（现名 \"{}\"）", key, self.handlers[&key])));
        }
        self.handlers.insert(key, name.to_string());
        self.per_key.insert(key, HandlerTimeStat::default());
        self.enrolled_total += 1;
        Ok(())
    }

    /// 时长观测：per-key + 全局双写直方图，超时红账联动。
    ///
    /// 返回本次观测的步数（预算断言消费）。
    pub fn observe(&mut self, key: u32, duration_us: u64, tick: u64) -> Result<u64, (u32, String)> {
        let mut steps: u64 = 0;
        // 1）在册检查（1 步）。
        steps += 1;
        if !self.handlers.contains_key(&key) {
            return Err((XC_UNKNOWN_HANDLER, format!("键 #{} 未入册——无主观测不入账", key)));
        }
        // 2）时长合法性（0 拒——观测必有非零时长）。
        steps += 1;
        if duration_us == 0 {
            return Err((XC_DURATION_RANGE, format!("键 #{} 零时长——观测值非法", key)));
        }
        // 3）桶定位（至多 17 比较为 17 步）。
        let b = bucket_of(duration_us);
        steps += bucket_of_steps(duration_us);
        // 4）per-key 账写入（5 步：直方图/total/over/min-max/tick）。
        steps += 5;
        let stat = match self.per_key.get_mut(&key) {
            Some(s) => s,
            None => return Err((XC_UNKNOWN_HANDLER, format!("键 #{} 册账失偶", key))),
        };
        if let Some(prev) = stat.last_tick {
            if tick < prev {
                return Err((XC_TICK_REGRESS, format!("键 #{} tick 回退（{} < {}）", key, tick, prev)));
            }
        }
        stat.hist[b] = stat.hist[b].wrapping_add(1);
        stat.total = stat.total.wrapping_add(1);
        let is_over = duration_us > TIMEOUT_US;
        if is_over {
            stat.over_count = stat.over_count.wrapping_add(1);
        }
        let upd_min = match stat.min_us {
            Some(m) if m <= duration_us => m,
            _ => duration_us,
        };
        let upd_max = match stat.max_us {
            Some(m) if m >= duration_us => m,
            _ => duration_us,
        };
        stat.min_us = Some(upd_min);
        stat.max_us = Some(upd_max);
        stat.last_tick = Some(tick);
        // 5）全局账写入（3 步）。
        steps += 3;
        self.global_hist[b] = self.global_hist[b].wrapping_add(1);
        self.observe_total = self.observe_total.wrapping_add(1);
        if is_over {
            self.timeout_red_total = self.timeout_red_total.wrapping_add(1);
        }
        // 6）预算断言（1 步——<100ns 判据的确定性代理）。
        steps += 1;
        if steps > OBS_BUDGET_STEPS {
            return Err((
                XC_OBS_BUDGET,
                format!("观测步数 {} 超预算 {}——开销断言破", steps, OBS_BUDGET_STEPS),
            ));
        }
        Ok(steps)
    }

    /// 直方图守恒校验：per-key Σ桶==total、全局 Σ桶==observe_total、
    /// Σper-key hist==global_hist（三重——分布面不失恒）。
    pub fn histo_conservation(&self) -> Result<(), (u32, String)> {
        let mut acc = [0u64; N_BUCKETS];
        for (k, s) in self.per_key.iter() {
            if s.hist_sum() != s.total {
                return Err((
                    XC_HISTO_IMBALANCE,
                    format!("键 #{} 直方图失恒：Σ桶 {} ≠ total {}", k, s.hist_sum(), s.total),
                ));
            }
            for i in 0..N_BUCKETS {
                acc[i] = acc[i].wrapping_add(s.hist[i]);
            }
        }
        for i in 0..N_BUCKETS {
            if acc[i] != self.global_hist[i] {
                return Err((
                    XC_HISTO_IMBALANCE,
                    format!("桶 {} 账实不符：Σper-key {} ≠ 全局 {}", i, acc[i], self.global_hist[i]),
                ));
            }
        }
        let gsum = self.global_hist.iter().fold(0u64, |a, b| a.wrapping_add(*b));
        if gsum != self.observe_total {
            return Err((
                XC_HISTO_IMBALANCE,
                format!("全局失恒：Σ桶 {} ≠ 观测总数 {}", gsum, self.observe_total),
            ));
        }
        Ok(())
    }

    /// 超时清单自动产出：>阈值 handler 的（键/名/次数/最值）行集。
    ///
    /// 判据"超时红账联动（>100µs handler 清单自动产出）"的正主。
    pub fn timeout_list(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (k, s) in self.per_key.iter() {
            if s.over_count > 0 {
                let name = self.handlers.get(k).cloned().unwrap_or_default();
                out.push(format!(
                    "超时 键#{} 名=\"{}\" 次={} 最长={}µs 最短={}µs",
                    k,
                    name,
                    s.over_count,
                    s.max_us.unwrap_or(0),
                    s.min_us.unwrap_or(0)
                ));
            }
        }
        out
    }

    /// 桶行报告（全局直方图——F0899 人话体例）。
    pub fn histo_report(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..N_BUCKETS {
            if self.global_hist[i] > 0 {
                let label = if i == 0 {
                    "<=1us".to_string()
                } else if i == N_BUCKETS - 1 {
                    ">65536us".to_string()
                } else {
                    format!("({},{}]us", 1u64 << (i - 1), 1u64 << i)
                };
                out.push(format!("桶={} 计数={}", label, self.global_hist[i]));
            }
        }
        out
    }
}

/// 桶定位步数（与 bucket_of 同口径——显性循环计数）。
fn bucket_of_steps(duration_us: u64) -> u64 {
    for i in 0..(N_BUCKETS - 1) {
        if duration_us <= (1u64 << i) {
            return i as u64 + 1;
        }
    }
    N_BUCKETS as u64 - 1
}

// ---------------------------------------------------------------------------
// §4 F0899 口径导出（指标三元组——µs 单位主力）
// ---------------------------------------------------------------------------

/// 指标单位闭集（F0913 schema 纪律复用声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricUnit {
    /// 事件计数。
    Count,
    /// 微秒。
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

/// 更新周期闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdatePeriod {
    /// 每 tick。
    PerTick,
    /// 每巡检。
    PerSweep,
    /// 按需。
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

/// 指标 schema 校验（F0899 口径：名/单位/周期——与 F0913 同规）。
pub fn metric_schema_ok(name: &str, unit: MetricUnit, period: UpdatePeriod) -> Result<(), (u32, String)> {
    if name.is_empty() || name.len() > 32 {
        return Err((XC_METRIC_SCHEMA, format!("指标名 \"{}\" 长度越界", name)));
    }
    for b in name.bytes() {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-';
        if !ok {
            return Err((XC_METRIC_SCHEMA, format!("指标名含非法字符 '{}'", b as char)));
        }
    }
    let _ = (unit.name(), period.name()); // 闭集显性化
    Ok(())
}

/// 指标行。
pub struct MetricRow {
    /// 名。
    pub name: String,
    /// 单位。
    pub unit: MetricUnit,
    /// 周期。
    pub period: UpdatePeriod,
    /// 值。
    pub value: u64,
}

impl MetricRow {
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

/// 导出：时长指标集（先过 schema 闸）。
pub fn export_metrics(led: &TimeLedger) -> Result<Vec<String>, (u32, String)> {
    let rows = [
        MetricRow {
            name: "handler.observe.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerTick,
            value: led.observe_total,
        },
        MetricRow {
            name: "handler.timeout.total".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::PerSweep,
            value: led.timeout_red_total,
        },
        MetricRow {
            name: "handler.enrolled.keys".to_string(),
            unit: MetricUnit::Count,
            period: UpdatePeriod::OnDemand,
            value: led.enrolled_total,
        },
    ];
    let mut out = Vec::new();
    for r in rows.iter() {
        metric_schema_ok(&r.name, r.unit, r.period)?;
        out.push(r.line());
    }
    Ok(out)
}

/// 导出确定性对拍。
pub fn export_determinism(led: &TimeLedger) -> Result<usize, (u32, String)> {
    let a = export_metrics(led)?;
    let b = export_metrics(led)?;
    if a != b {
        return Err((XC_EXPORT_DRIFT, "导出两次不等——确定性失守".to_string()));
    }
    Ok(a.len())
}

// ---------------------------------------------------------------------------
// §5 LCG + FNV
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

/// 时长账快照。
pub struct TimeSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl TimeSnap {
    /// 采样。
    pub fn take(led: &TimeLedger) -> TimeSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&led.observe_total.to_le_bytes());
        buf.extend_from_slice(&led.timeout_red_total.to_le_bytes());
        buf.extend_from_slice(&led.enrolled_total.to_le_bytes());
        for v in led.global_hist.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for (k, s) in led.per_key.iter() {
            buf.extend_from_slice(&k.to_le_bytes());
            for v in s.hist.iter() {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            buf.extend_from_slice(&s.total.to_le_bytes());
            buf.extend_from_slice(&s.over_count.to_le_bytes());
        }
        TimeSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "册={} 观测={} 超时红={}",
                led.per_key.len(),
                led.observe_total,
                led.timeout_red_total
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §6 十万压测（per-handler 独立对拍 + 守恒 + 超时清单 + 预算）
// ---------------------------------------------------------------------------

/// 压测观测数。
pub const STRESS_OBS: u32 = 100_000;
/// 压测 handler 数。
pub const STRESS_KEYS: u32 = 8;

/// 十万观测压测：LCG 生成时长分布（含超时簇），逐键逐桶对拍。
pub fn stress_100k() -> Result<String, (u32, String)> {
    let mut led = TimeLedger::new();
    let mut rng = Lcg::new(0x11E4_0B57);
    for i in 0..STRESS_KEYS {
        led.enroll(i, &format!("hdl.{}", i))?;
    }
    // 注入账：per-key 直方图期望（桶级精确对拍）。
    let mut expect: BTreeMap<u32, HandlerTimeStat> = BTreeMap::new();
    for i in 0..STRESS_KEYS {
        expect.insert(i, HandlerTimeStat::default());
    }
    let mut max_steps: u64 = 0;
    for e in 0..STRESS_OBS {
        let key = rng.below(STRESS_KEYS);
        // 时长分布：70% 短跑（1..64µs）、20% 中段（64..1024）、
        // 10% 超时簇（128..20000µs）——超时红账有稳定原料。
        let roll = rng.below(100);
        let dur = if roll < 70 {
            1 + rng.below(63) as u64
        } else if roll < 90 {
            64 + rng.below(960) as u64
        } else {
            128 + rng.below(19872) as u64
        };
        let tick = e as u64;
        let steps = led.observe(key, dur, tick)?;
        if steps > max_steps {
            max_steps = steps;
        }
        // 期望账同步（与 observe 同口径逐槽写）。
        let b = bucket_of(dur);
        let exp = match expect.get_mut(&key) {
            Some(v) => v,
            None => return Err((XC_UNKNOWN_HANDLER, format!("期望账缺键 #{}", key))),
        };
        exp.hist[b] += 1;
        exp.total += 1;
        if dur > TIMEOUT_US {
            exp.over_count += 1;
        }
        let m = match exp.min_us {
            Some(v) if v <= dur => v,
            _ => dur,
        };
        let mx = match exp.max_us {
            Some(v) if v >= dur => v,
            _ => dur,
        };
        exp.min_us = Some(m);
        exp.max_us = Some(mx);
    }
    // 1）逐键逐桶对拍（零串账——直方图级）。
    for (k, exp) in expect.iter() {
        match led.per_key.get(k) {
            Some(s) => {
                for i in 0..N_BUCKETS {
                    if s.hist[i] != exp.hist[i] {
                        return Err((
                            XC_HISTO_IMBALANCE,
                            format!("键 #{} 桶 {} 串账：账 {} ≠ 注入 {}", k, i, s.hist[i], exp.hist[i]),
                        ));
                    }
                }
                if s.over_count != exp.over_count {
                    return Err((
                        XC_TIMEOUT_RED,
                        format!("键 #{} 超时计数失准：{} ≠ {}", k, s.over_count, exp.over_count),
                    ));
                }
            }
            None => return Err((XC_UNKNOWN_HANDLER, format!("键 #{} 账丢失", k))),
        }
    }
    // 2）三重直方图守恒。
    led.histo_conservation()?;
    // 3）预算断言：全程单次观测步数 ≤ 预算。
    if max_steps > OBS_BUDGET_STEPS {
        return Err((
            XC_OBS_BUDGET,
            format!("压测最大步数 {} 超预算 {}", max_steps, OBS_BUDGET_STEPS),
        ));
    }
    // 4）超时清单非空且行数==超时键数。
    let list = led.timeout_list();
    let timeout_keys = expect.values().filter(|s| s.over_count > 0).count();
    if list.len() != timeout_keys {
        return Err((
            XC_TIMEOUT_RED,
            format!("超时清单 {} 行 ≠ 超时键数 {}", list.len(), timeout_keys),
        ));
    }
    // 5）导出确定性。
    let rows = export_determinism(&led)?;
    // 6）快照稳定。
    let s0 = TimeSnap::take(&led);
    let s1 = TimeSnap::take(&led);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "压测账快照不稳定".to_string()));
    }
    Ok(format!(
        "十万压测全绿：{} 观测 8 键逐桶对拍平 三重守恒过 预算内(峰值 {} 步) 超时清单 {} 行 导出 {} 行 快照 {:#010x}",
        STRESS_OBS, max_steps, list.len(), rows, s0.hash
    ))
}

// ---------------------------------------------------------------------------
// §7 注入面
// ---------------------------------------------------------------------------

/// 八闸注入：无主/重复/零时长/超预算代理/tick 回退/守恒破缺/超时
/// 清单/schema 违例。
pub fn inject_gates() -> Result<String, (u32, String)> {
    // 1 无主观测。
    let mut led = TimeLedger::new();
    let r = led.observe(9, 5, 0);
    if !matches!(r, Err((XC_UNKNOWN_HANDLER, _))) {
        return Err((XC_UNKNOWN_HANDLER, "无主观测未拦".to_string()));
    }
    // 2 重复入册。
    let mut led = TimeLedger::new();
    led.enroll(1, "a")?;
    let r = led.enroll(1, "b");
    if !matches!(r, Err((XC_DUP_HANDLER, _))) {
        return Err((XC_DUP_HANDLER, "重复入册未拦".to_string()));
    }
    // 3 零时长。
    let mut led = TimeLedger::new();
    led.enroll(2, "b")?;
    let r = led.observe(2, 0, 0);
    if !matches!(r, Err((XC_DURATION_RANGE, _))) {
        return Err((XC_DURATION_RANGE, "零时长未拦".to_string()));
    }
    // 4 tick 回退。
    let mut led = TimeLedger::new();
    led.enroll(3, "c")?;
    led.observe(3, 10, 100)?;
    let r = led.observe(3, 10, 50);
    if !matches!(r, Err((XC_TICK_REGRESS, _))) {
        return Err((XC_TICK_REGRESS, "tick 回退未拦".to_string()));
    }
    // 5 守恒破缺（人工抽走直方图一桶——三重校验必红）。
    let mut led = TimeLedger::new();
    led.enroll(4, "d")?;
    led.observe(4, 3, 1)?;
    led.observe(4, 5, 2)?;
    if let Some(s) = led.per_key.get_mut(&4) {
        s.hist[1] = s.hist[1].saturating_sub(1);
    }
    let r = led.histo_conservation();
    if !matches!(r, Err((XC_HISTO_IMBALANCE, _))) {
        return Err((XC_HISTO_IMBALANCE, "直方图破缺未检出".to_string()));
    }
    // 6 超时清单联动（>100µs 落红账且清单产出）。
    let mut led = TimeLedger::new();
    led.enroll(5, "fast")?;
    led.enroll(6, "slow")?;
    for t in 0..10u64 {
        led.observe(5, 20, t)?; // 全短跑
    }
    for t in 0..10u64 {
        led.observe(6, 5000, t)?; // 全超时
    }
    let list = led.timeout_list();
    let ok_list = list.len() == 1 && list[0].contains("键#6") && list[0].contains("slow");
    if !ok_list {
        return Err((
            XC_TIMEOUT_RED,
            format!("超时清单失准：{:?}", list),
        ));
    }
    if led.timeout_red_total != 10 {
        return Err((
            XC_TIMEOUT_RED,
            format!("红账计数 {} ≠ 10", led.timeout_red_total),
        ));
    }
    // 7 schema 违例（大写）。
    let r = metric_schema_ok("Bad.Metric", MetricUnit::Micros, UpdatePeriod::PerTick);
    if !matches!(r, Err((XC_METRIC_SCHEMA, _))) {
        return Err((XC_METRIC_SCHEMA, "schema 大写未拦".to_string()));
    }
    // 8 桶定位口径自检（边界值：1→桶0、2→桶1、3→桶1、65536→桶16、65537→桶17）。
    let cases = [(1u64, 0usize), (2, 1), (3, 1), (4, 2), (65536, 16), (65537, 17)];
    for (d, want) in cases.iter() {
        if bucket_of(*d) != *want {
            return Err((
                XC_HISTO_IMBALANCE,
                format!("桶定位口径失守：{}µs 落桶 {} 期望 {}", d, bucket_of(*d), want),
            ));
        }
    }
    Ok("八闸全拦（无主/重复/零时长/tick 回退/守恒破缺/超时清单/schema/桶口径）".to_string())
}

// ---------------------------------------------------------------------------
// §8 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：八闸注入 + 十万压测 + 导出确定性 + 快照稳定。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(inject_gates()?);
    parts.push(stress_100k()?);
    // 导出确定性（空账基线）。
    let empty = TimeLedger::new();
    let rows = export_determinism(&empty)?;
    parts.push(format!("导出确定性 {} 行逐字一致（空账基线）", rows));
    // 快照稳定性。
    let s0 = TimeSnap::take(&empty);
    let s1 = TimeSnap::take(&empty);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空账快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
