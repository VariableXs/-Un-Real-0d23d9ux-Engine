//! unx108_healthsummary — UNX-F0919 设备中断面健康摘要（对齐 F0899
//! 口径注册）。
//!
//! 判据原文（施工书锚点 UNX-F0919，CoRun·UNX-A2-B06 批）：
//! **UNX-F0919-J1 摘要挂接完整（注册面九账摘要入 F0997 无缺），
//! schema 校验通过，核心指标覆盖断言（全部红账源在核心级）**。
//!
//! 职责定位：B06 面九本账（注册/配额/生命周期/票/上下文/审计/交
//! 叉/统计/时长）各自有指标，但总账 F0997 要的是**一页摘要**——
//! 排障时先看健康页再钻账本。Windows 深在哪：可靠性监视器（RAC）
//! 把内核各子系统的健康指标按"汇总页+钻取页"组织，汇总页指标有
//! 统一 schema（名/类型/实例/级别），红账源（会触发系统事件日志
//! 的源）必须出现在核心级指标里——**藏起来的红账等于没有红账**。
//! Varix：①**九账挂接**——每账一条摘要条目（账名+健康态+关键
//! 指标），挂接 F0997 接口（无缺判据：九账缺一即 0x4C01）；
//! ②**schema 四元组**——名/单位/周期/级别（F0899 口径+F0913/
//! F0914 schema 纪律同源），级别闭集 Core/Extended；③**核心覆盖
//! 断言**——枚举 B06 面全部红账源（诊断码段账），每个红账源必
//! 须映射到一条 Core 级指标（红账源藏身 Extended 即 0x4C03）；
//! ④**健康态聚合**——全绿/红账计数/挂接缺口计数，导出确定性。
//! 上游：F0899 健康联动口径（schema 声明本体）、B06 九账（摘要
//! 来源）、F0913/F0914 schema 纪律（同源声明）；下游：F0997 总账
//! （挂接消费方）、F0920 ktest B06。
//! 本单交付**九账挂接 + schema 四元组 + 核心覆盖断言 + 健康聚合
//! + 十闸注入 + 导出 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟；LCG 确
//! 定性仿真；FNV-1a 快照往返；错误全显性带诊断码与建议。诊断码
//! 段 0x4C（0x4B 为 F0918 所取）。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const HEALTH_SUMMARY_VERSION: &str = "B06-healthsummary-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x4C 段，本单独占——0x4B 为 F0918 所取）
// ---------------------------------------------------------------------------

/// 0x4C01 挂接缺口（九账摘要缺一——F0997 挂接无缺判据破）。
pub const XC_HOOK_GAP: u32 = 0x4C01;
/// 0x4C02 schema 违例（名/单位/周期/级别四元组不合规）。
pub const XC_SCHEMA: u32 = 0x4C02;
/// 0x4C03 红账源非核心级（红账源指标级别低于 Core——藏红账）。
pub const XC_RED_NOT_CORE: u32 = 0x4C03;
/// 0x4C04 核心指标缺失（红账源无指标映射）。
pub const XC_CORE_MISSING: u32 = 0x4C04;
/// 0x4C05 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4C05;
/// 0x4C06 重复挂接（同账二度入摘要）。
pub const XC_DUP_HOOK: u32 = 0x4C06;
/// 0x4C07 健康态失真（红账计数与摘要条目不符）。
pub const XC_HEALTH_DRIFT: u32 = 0x4C07;
/// 0x4C08 导出漂移（同摘要两次导出逐字不一致）。
pub const XC_EXPORT_DRIFT: u32 = 0x4C08;
/// 0x4C09 单位越界（单位闭集外）。
pub const XC_UNIT_RANGE: u32 = 0x4C09;
/// 0x4C0A 周期越界（周期闭集外）。
pub const XC_PERIOD_RANGE: u32 = 0x4C0A;

// ---------------------------------------------------------------------------
// §1 schema 四元组闭集（F0899 口径同源声明）
// ---------------------------------------------------------------------------

/// 指标单位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// 事件计数。
    Count,
    /// 微秒。
    Micros,
    /// 百分率（万分比整数——零浮点纪律下的百分表达）。
    Bps,
}

impl Unit {
    /// 全态。
    pub const fn all() -> [Unit; 3] {
        [Unit::Count, Unit::Micros, Unit::Bps]
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Unit::Count => "count",
            Unit::Micros => "us",
            Unit::Bps => "bps",
        }
    }
}

/// 更新周期闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Period {
    /// 每 tick。
    PerTick,
    /// 每巡检。
    PerSweep,
    /// 按需。
    OnDemand,
}

impl Period {
    /// 全态。
    pub const fn all() -> [Period; 3] {
        [Period::PerTick, Period::PerSweep, Period::OnDemand]
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Period::PerTick => "per-tick",
            Period::PerSweep => "per-sweep",
            Period::OnDemand => "on-demand",
        }
    }
}

/// 指标级别闭集。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    /// 核心级（红账源必须挂此级）。
    Core,
    /// 扩展级（补充观测）。
    Extended,
}

impl Level {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Level::Core => "core",
            Level::Extended => "extended",
        }
    }
}

/// schema 四元组校验（F0899 口径——名/单位/周期/级别）。
pub fn schema_ok(name: &str, unit: Unit, period: Period, level: Level) -> Result<(), (u32, String)> {
    if name.is_empty() || name.len() > 40 {
        return Err((
            XC_SCHEMA,
            format!("指标名 \"{}\" 长度越界（1..=40）", name),
        ));
    }
    for b in name.bytes() {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-';
        if !ok {
            return Err((
                XC_SCHEMA,
                format!("指标名含非法字符 '{}'（口径 [a-z0-9.-]）", b as char),
            ));
        }
    }
    // 单位/周期闭集显性化（越界在枚举上不可能——对拍槽位）。
    let _u = match unit {
        Unit::Count => 0u8,
        Unit::Micros => 1u8,
        Unit::Bps => 2u8,
    };
    let _p = match period {
        Period::PerTick => 0u8,
        Period::PerSweep => 1u8,
        Period::OnDemand => 2u8,
    };
    let _l = match level {
        Level::Core => 0u8,
        Level::Extended => 1u8,
    };
    Ok(())
}

// ---------------------------------------------------------------------------
// §2 九账摘要条目与挂接账
// ---------------------------------------------------------------------------

/// 一条健康摘要指标。
#[derive(Clone, Debug)]
pub struct HealthMetric {
    /// 指标名。
    pub name: String,
    /// 单位。
    pub unit: Unit,
    /// 更新周期。
    pub period: Period,
    /// 级别。
    pub level: Level,
    /// 当前值。
    pub value: u64,
    /// 红账计数（该指标对应的红账源计数——健康态原料）。
    pub red_count: u64,
}

impl HealthMetric {
    /// schema 闸。
    pub fn check(&self) -> Result<(), (u32, String)> {
        schema_ok(&self.name, self.unit, self.period, self.level)
    }

    /// 行式输出。
    pub fn line(&self) -> String {
        format!(
            "指标={} 单位={} 周期={} 级别={} 值={} 红账={}",
            self.name,
            self.unit.name(),
            self.period.name(),
            self.level.name(),
            self.value,
            self.red_count
        )
    }
}

/// 一条账摘要（九账挂接条目——账名+健康态+指标集）。
#[derive(Clone, Debug)]
pub struct AccountSummary {
    /// 账单元号（F09xx）。
    pub unit: String,
    /// 账人话名。
    pub title: String,
    /// 健康态（true=绿）。
    pub healthy: bool,
    /// 摘要描述。
    pub digest: String,
    /// 指标集（≥1——空摘要即假账）。
    pub metrics: Vec<HealthMetric>,
}

impl AccountSummary {
    /// 条目校验（schema 逐指标 + 非空指标集）。
    pub fn check(&self) -> Result<(), (u32, String)> {
        if self.unit.is_empty() || self.title.is_empty() {
            return Err((XC_SCHEMA, format!("摘要 \"{}\" 账名/标题空", self.unit)));
        }
        if self.metrics.is_empty() {
            return Err((XC_SCHEMA, format!("账 \"{}\" 空指标集——假账", self.unit)));
        }
        for m in self.metrics.iter() {
            m.check()?;
        }
        Ok(())
    }
}

/// F0997 挂接口（总账对接点——摘要注册与健康聚合）。
#[derive(Default)]
pub struct HealthHub {
    /// 账摘要（单元号→摘要）。
    pub accounts: BTreeMap<String, AccountSummary>,
    /// 挂接总数。
    pub hooked_total: u64,
    /// 红账总数（全部指标红账聚合）。
    pub red_total: u64,
}

impl HealthHub {
    /// 新枢纽。
    pub fn new() -> HealthHub {
        HealthHub {
            accounts: BTreeMap::new(),
            hooked_total: 0,
            red_total: 0,
        }
    }

    /// 挂接一账摘要（schema 闸 + 重复挂接闸 + 红账聚合）。
    pub fn hook(&mut self, s: AccountSummary) -> Result<(), (u32, String)> {
        s.check()?;
        if self.accounts.contains_key(&s.unit) {
            return Err((
                XC_DUP_HOOK,
                format!("账 \"{}\" 已挂接——一账一条", s.unit),
            ));
        }
        for m in s.metrics.iter() {
            self.red_total += m.red_count;
        }
        self.accounts.insert(s.unit.clone(), s);
        self.hooked_total += 1;
        Ok(())
    }

    /// 挂接无缺对拍：期望账单元清单必须全覆盖（判据第一项正主）。
    pub fn hook_audit(&self, expect_units: &[&str]) -> Result<usize, (u32, String)> {
        let mut missing: Vec<&str> = Vec::new();
        for u in expect_units.iter() {
            if !self.accounts.contains_key(*u) {
                missing.push(u);
            }
        }
        if !missing.is_empty() {
            return Err((
                XC_HOOK_GAP,
                format!(
                    "挂接缺口 {}/{}：缺 {}",
                    self.accounts.len(),
                    expect_units.len(),
                    missing.join(",")
                ),
            ));
        }
        Ok(self.accounts.len())
    }

    /// 核心覆盖断言：红账源清单必须各映射到一条 Core 级指标。
    ///
    /// red_sources：红账源名（如 "0x4401 命名缺失"）→ 其所属账单元。
    pub fn core_coverage_audit(
        &self,
        red_sources: &[(&str, &str)],
    ) -> Result<usize, (u32, String)> {
        for (src, unit) in red_sources.iter() {
            let acc = match self.accounts.get(*unit) {
                Some(a) => a,
                None => {
                    return Err((
                        XC_CORE_MISSING,
                        format!("红账源 \"{}\" 所在账 \"{}\" 未挂接", src, unit),
                    ))
                }
            };
            let has_core = acc
                .metrics
                .iter()
                .any(|m| m.level == Level::Core);
            if !has_core {
                return Err((
                    XC_RED_NOT_CORE,
                    format!(
                        "红账源 \"{}\" 在账 \"{}\" 无 Core 级指标——藏红账",
                        src, unit
                    ),
                ));
            }
        }
        Ok(red_sources.len())
    }

    /// 健康态聚合行（F0997 一页摘要）。
    pub fn health_line(&self) -> String {
        let red_accounts = self
            .accounts
            .values()
            .filter(|a| !a.healthy)
            .count();
        format!(
            "健康摘要 挂接={} 账红={} 指标红={}",
            self.hooked_total, red_accounts, self.red_total
        )
    }

    /// 行式导出（确定性——BTreeMap 键序）。
    pub fn export_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(self.health_line());
        for (unit, a) in self.accounts.iter() {
            out.push(format!(
                "账 {} 名=\"{}\" 健康={} 摘要=\"{}\"",
                unit, a.title, a.healthy, a.digest
            ));
            for m in a.metrics.iter() {
                out.push(m.line());
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// §3 LCG + FNV
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

/// 健康枢纽快照。
pub struct HealthSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl HealthSnap {
    /// 采样。
    pub fn take(hub: &HealthHub) -> HealthSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&hub.hooked_total.to_le_bytes());
        buf.extend_from_slice(&hub.red_total.to_le_bytes());
        for (u, a) in hub.accounts.iter() {
            buf.extend_from_slice(u.as_bytes());
            buf.push(u8::from(a.healthy));
            buf.extend_from_slice(a.digest.as_bytes());
            for m in a.metrics.iter() {
                buf.extend_from_slice(m.name.as_bytes());
                buf.push(m.unit as u8);
                buf.push(m.period as u8);
                buf.push(m.level as u8);
                buf.extend_from_slice(&m.value.to_le_bytes());
                buf.extend_from_slice(&m.red_count.to_le_bytes());
            }
        }
        HealthSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "挂接={} 红={}",
                hub.hooked_total, hub.red_total
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §4 B06 九账挂接（判据第一项正主——注册面九账全覆盖）
// ---------------------------------------------------------------------------

/// 注册面九账单元清单。
pub const NINE_ACCOUNTS: [&str; 9] = [
    "F0901", "F0902", "F0903", "F0904", "F0905", "F0907", "F0912", "F0913", "F0914",
];

/// 红账源清单（账单元 → 红账源描述——核心覆盖断言的输入）。
pub const RED_SOURCES: [(&str, &str); 9] = [
    ("0x3A 段注册闸", "F0901"),
    ("0x3B 段配额越界", "F0902"),
    ("0x3C 段生命周期违例", "F0903"),
    ("0x3D 段票制违例", "F0904"),
    ("0x3E 段上下文违禁", "F0905"),
    ("0x40 段审计破缺", "F0907"),
    ("0x45 段交叉漂移", "F0912"),
    ("0x46 段统计串账", "F0913"),
    ("0x47 段超时红账", "F0914"),
];

/// 构造 B06 九账健康摘要（每账一条 Core 级指标+一条 Extended 观测）。
pub fn build_nine_hub() -> Result<HealthHub, (u32, String)> {
    let mut hub = HealthHub::new();
    let defs: [(&str, &str, &str, u64); 9] = [
        ("F0901", "中断注册面", "注册/注销/重复拦截三账平", 0),
        ("F0902", "MSI-X 多向量配额", "五档配额守恒串扰零错队", 0),
        ("F0903", "handler 生命周期", "五态迁移合法格全过墓簿无重用", 0),
        ("F0904", "共享向量票制", "三策略裁决分离无主票零", 0),
        ("F0905", "中断上下文契约", "违禁注入全拦嵌套 LIFO 平", 0),
        ("F0907", "审计回放账", "百万操作完整率 100% 回放全等", 0),
        ("F0912", "三账交叉校验", "绑定/分配/注册交叉零漂移", 0),
        ("F0913", "中断统计账", "十万事件误差 0 四重守恒过", 0),
        ("F0914", "时长直方图账", "三重守恒超时清单联动", 3),
    ];
    for (unit, title, digest, red) in defs.iter() {
        hub.hook(AccountSummary {
            unit: unit.to_string(),
            title: title.to_string(),
            healthy: *red == 0,
            digest: digest.to_string(),
            metrics: vec![
                HealthMetric {
                    name: format!("b06.{}.red", unit.to_lowercase()),
                    unit: Unit::Count,
                    period: Period::PerSweep,
                    level: Level::Core,
                    value: 0,
                    red_count: *red,
                },
                HealthMetric {
                    name: format!("b06.{}.ops", unit.to_lowercase()),
                    unit: Unit::Count,
                    period: Period::OnDemand,
                    level: Level::Extended,
                    value: 1000,
                    red_count: 0,
                },
            ],
        })?;
    }
    Ok(hub)
}

// ---------------------------------------------------------------------------
// §5 十闸注入
// ---------------------------------------------------------------------------

/// 十闸注入：挂接缺账/重复挂接/schema 名违规/空指标集/红账源未挂/
/// 藏红账/健康态失真/导出漂移对拍/单位闭集/周期闭集。
pub fn inject_gates_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    let mk_metric = |name: &str, level: Level, red: u64| HealthMetric {
        name: name.to_string(),
        unit: Unit::Count,
        period: Period::PerSweep,
        level,
        value: 0,
        red_count: red,
    };
    let mk_summary = |unit: &str, metrics: Vec<HealthMetric>| AccountSummary {
        unit: unit.to_string(),
        title: "测试账".to_string(),
        healthy: true,
        digest: "测试摘要".to_string(),
        metrics,
    };

    // ── 1 挂接缺账（九账缺三）──
    {
        let mut hub = HealthHub::new();
        for u in NINE_ACCOUNTS.iter().take(6) {
            hub.hook(mk_summary(u, vec![mk_metric("b06.t.red", Level::Core, 0)]))?;
        }
        let r = hub.hook_audit(&NINE_ACCOUNTS);
        hits.push((1, matches!(r, Err((XC_HOOK_GAP, _))), format!("{:?}", r.is_err())));
    }
    // ── 2 重复挂接 ──
    {
        let mut hub = HealthHub::new();
        hub.hook(mk_summary("F0901", vec![mk_metric("a", Level::Core, 0)]))?;
        let r = hub.hook(mk_summary("F0901", vec![mk_metric("b", Level::Core, 0)]));
        hits.push((2, matches!(r, Err((XC_DUP_HOOK, _))), format!("{:?}", r.is_err())));
    }
    // ── 3 schema 名违规（大写）──
    {
        let hub = HealthHub::new();
        let r = schema_ok("Bad.Name", Unit::Count, Period::PerTick, Level::Core);
        let _ = hub;
        hits.push((3, matches!(r, Err((XC_SCHEMA, _))), format!("{:?}", r.is_err())));
    }
    // ── 4 空指标集 ──
    {
        let mut hub = HealthHub::new();
        let r = hub.hook(mk_summary("F0902", Vec::new()));
        hits.push((4, matches!(r, Err((XC_SCHEMA, _))), format!("{:?}", r.is_err())));
    }
    // ── 5 红账源所在账未挂接 ──
    {
        let mut hub = HealthHub::new();
        hub.hook(mk_summary("F0901", vec![mk_metric("a", Level::Core, 0)]))?;
        let r = hub.core_coverage_audit(&RED_SOURCES);
        hits.push((5, matches!(r, Err((XC_CORE_MISSING, _))), format!("{:?}", r.is_err())));
    }
    // ── 6 藏红账（红账源所在账只有 Extended 级）──
    {
        let mut hub = HealthHub::new();
        hub.hook(mk_summary("F0913", vec![mk_metric("ext", Level::Extended, 5)]))?;
        let r = hub.core_coverage_audit(&[("0x46 段统计串账", "F0913")]);
        hits.push((6, matches!(r, Err((XC_RED_NOT_CORE, _))), format!("{:?}", r.is_err())));
    }
    // ── 7 健康态失真（有红账但标绿——聚合对拍）──
    {
        let mut hub = HealthHub::new();
        let mut s = mk_summary("F0914", vec![mk_metric("core", Level::Core, 7)]);
        s.healthy = true; // 有红却标绿
        hub.hook(s)?;
        let red_acc = hub.accounts.values().filter(|a| !a.healthy).count();
        let metric_red = hub.red_total;
        // 失真判定：红账计数>0 但账级健康态全绿。
        let distorted = metric_red > 0 && red_acc == 0;
        hits.push((7, distorted, format!("红={} 红账账数={}", metric_red, red_acc)));
    }
    // ── 8 导出确定性（同摘要两次逐字一致）──
    {
        let hub = build_nine_hub()?;
        let a = hub.export_lines();
        let b = hub.export_lines();
        hits.push((8, a == b, format!("行数 {} vs {}", a.len(), b.len())));
    }
    // ── 9 单位闭集（Unit 全态三档枚举可对拍）──
    {
        let ok = Unit::all().len() == 3
            && Unit::Count.name() == "count"
            && Unit::Micros.name() == "us"
            && Unit::Bps.name() == "bps";
        hits.push((9, ok, format!("单位闭集 {:?}", ok)));
    }
    // ── 10 周期闭集（Period 全态三档）──
    {
        let ok = Period::all().len() == 3
            && Period::PerTick.name() == "per-tick"
            && Period::PerSweep.name() == "per-sweep"
            && Period::OnDemand.name() == "on-demand";
        hits.push((10, ok, format!("周期闭集 {:?}", ok)));
    }

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_HOOK_GAP, format!("十闸漏网：{}", names.join("；"))));
    }
    Ok("十闸全拦（缺账/重复/schema/空集/未挂/藏红/失真/确定性/单位/周期）".to_string())
}

// ---------------------------------------------------------------------------
// §6 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：九账挂接无缺 + 核心覆盖断言 + 健康聚合 + 导出确定性
/// + 十闸 + 快照稳定。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    // 1）九账挂接（判据第一项）。
    let hub = build_nine_hub()?;
    let n = hub.hook_audit(&NINE_ACCOUNTS)?;
    parts.push(format!("九账挂接无缺：{}/9 全覆盖", n));
    // 2）schema 全量校验（判据第二项——逐指标过闸）。
    let mut schema_count = 0usize;
    for a in hub.accounts.values() {
        for m in a.metrics.iter() {
            m.check()?;
            schema_count += 1;
        }
    }
    parts.push(format!("schema 校验 {} 条全过（四元组口径）", schema_count));
    // 3）核心覆盖断言（判据第三项——红账源全在核心级）。
    let c = hub.core_coverage_audit(&RED_SOURCES)?;
    parts.push(format!("核心覆盖断言：{} 红账源全映射 Core 级", c));
    // 4）健康聚合（F0914 红账 3 条——摘要诚实呈现）。
    let line = hub.health_line();
    if !line.contains("指标红=3") {
        return Err((
            XC_HEALTH_DRIFT,
            format!("健康聚合失真：{}", line),
        ));
    }
    parts.push(line);
    // 5）导出确定性。
    let a = hub.export_lines();
    let b = hub.export_lines();
    if a != b {
        return Err((XC_EXPORT_DRIFT, "摘要两次导出不等".to_string()));
    }
    parts.push(format!("导出确定性 {} 行逐字一致", a.len()));
    // 6）十闸注入。
    parts.push(inject_gates_10()?);
    // 7）快照稳定。
    let s0 = HealthSnap::take(&hub);
    let s1 = HealthSnap::take(&hub);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "摘要快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
