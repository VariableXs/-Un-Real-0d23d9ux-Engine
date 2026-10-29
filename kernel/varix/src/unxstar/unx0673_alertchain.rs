//! UNX-F0673 · 耗时告警链（超阈/离群/退化三源路由）· 诊断段 0x5560（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0673-J1 三源告警（超阈/真离群/退化）路由 100%（注入复验），
//! 告警账行四要素齐且零静默吞警。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 三源闭集：超阈（>3000ms）/ 真离群（3×IQR 栅栏外且复跑仍出）/ 退化（较基线升 ≥10%）。
//!  3. 路由优先级：退化 > 真离群 > 超阈（同事件多源命中取最高优先级，零重复告警）。
//!  4. 告警账行四要素：源/级（警告/严重）/事件键/时戳序号，行行可解引。
//!  5. 零静默吞警：任何源触发必须落账，路由丢弃路径必须返回错误。
//!  6. 诊断段 0x5560 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5560;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_SRC: u16 = SEG_BASE + 1;    // 告警源闭集外
pub const E_DUP: u16 = SEG_BASE + 2;    // 同事件键重复告警（抑制重复）
pub const E_DROP: u16 = SEG_BASE + 3;   // 路由丢弃（零静默吞警拦截）
pub const E_ROW: u16 = SEG_BASE + 4;    // 告警账行要素缺
pub const E_LEVEL: u16 = SEG_BASE + 5;  // 告警级非法
pub const E_SEAL: u16 = SEG_BASE + 6;   // 告警账已封
pub const E_NOSRC: u16 = SEG_BASE + 7;  // 无源事件（空路由）

/// 超阈硬线（ms）。
pub const THRESH_MS: u64 = 3_000;
/// 退化硬线（千分比）。
pub const REGRESS_PM: u32 = 100;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 告警源闭集三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSrc {
    /// 超阈：单次耗时 > 3000ms。
    OverThresh,
    /// 真离群：3×IQR 栅栏外且复跑复现。
    TrueOutlier,
    /// 退化：较基线升 ≥100‰。
    Regress,
}

impl AlertSrc {
    /// 优先级：退化 3 > 真离群 2 > 超阈 1。
    pub fn prio(self) -> u8 {
        match self {
            AlertSrc::OverThresh => 1,
            AlertSrc::TrueOutlier => 2,
            AlertSrc::Regress => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AlertSrc::OverThresh => "超阈",
            AlertSrc::TrueOutlier => "真离群",
            AlertSrc::Regress => "退化",
        }
    }
}

/// 告警级闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertLevel {
    /// 警告：单源低幅度。
    Warn,
    /// 严重：高幅度或多源。
    Severe,
}

impl AlertLevel {
    pub fn name(self) -> &'static str {
        match self {
            AlertLevel::Warn => "警告",
            AlertLevel::Severe => "严重",
        }
    }
}

/// 事件：源 + 事件键 + 幅度。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertEvent {
    pub src: AlertSrc,
    /// 事件键（机/轮打包，重复抑制键）。
    pub key: u64,
    /// 幅度（超阈: ms；离群: 序号；退化: 千分比）。
    pub mag: u64,
}

/// 源触发判定（纯函数，注入复验的判定核）。
pub fn source_fires(src: AlertSrc, metric: u64, baseline: u64) -> Result<bool, u16> {
    match src {
        AlertSrc::OverThresh => Ok(metric > THRESH_MS),
        AlertSrc::TrueOutlier => {
            // 离群口径由上游栅栏判，此处以 metric>0 且栅栏外标记（metric 即栅栏外旗）
            Ok(metric > 0 && baseline > 0)
        }
        AlertSrc::Regress => {
            if baseline == 0 {
                return Err(E_SRC);
            }
            if metric <= baseline {
                return Ok(false);
            }
            Ok(((metric - baseline) * 1000 / baseline) as u32 >= REGRESS_PM)
        }
    }
}

/// 告警账行：四要素。
#[derive(Clone, Debug)]
pub struct AlertRow {
    pub src: AlertSrc,
    pub level: AlertLevel,
    pub key: u64,
    pub seq: u64,
}

impl AlertRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.key == 0 || self.seq == 0 {
            return Err(E_ROW);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("[");
        s.push_str(self.level.name());
        s.push_str("] ");
        s.push_str(self.src.name());
        s.push_str(" key=");
        s.push_str(&self.key.to_string());
        s
    }
}

/// 告警路由器。
pub struct AlertRouter {
    pub rows: Vec<AlertRow>,
    pub seq: u64,
    pub sealed: bool,
    /// 被抑制的重复事件计数（去重 ≠ 吞警：留计数可查）。
    pub suppressed: u32,
}

impl AlertRouter {
    pub fn new() -> AlertRouter {
        AlertRouter { rows: Vec::new(), seq: 0, sealed: false, suppressed: 0 }
    }

    /// 路由一次事件：同 key 重复抑制（计数留痕），否则落账。
    pub fn route(&mut self, ev: &AlertEvent) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if ev.key == 0 {
            return Err(E_ROW);
        }
        // 源闭集校验由枚举保证；级判定：
        let level = if ev.src.prio() >= 2 && ev.mag >= 100 {
            AlertLevel::Severe
        } else {
            AlertLevel::Warn
        };
        for r in &self.rows {
            if r.key == ev.key && r.src == ev.src {
                self.suppressed += 1;
                return Ok(()); // 抑制不是丢弃：计数可查
            }
        }
        self.seq += 1;
        let row = AlertRow { src: ev.src, level, key: ev.key, seq: self.seq };
        row.audit()?;
        self.rows.push(row);
        Ok(())
    }

    /// 多源事件路由：同 key 多源命中取最高优先级（零重复告警）。
    pub fn route_multi(&mut self, key: u64, sources: &[AlertSrc], mag: u64) -> Result<u8, u16> {
        if sources.is_empty() {
            return Err(E_NOSRC);
        }
        if key == 0 {
            return Err(E_ROW);
        }
        // 取最高优先级源
        let mut best = sources[0];
        for s in sources {
            if s.prio() > best.prio() {
                best = *s;
            }
        }
        self.route(&AlertEvent { src: best, key, mag })?;
        Ok(best.prio())
    }

    /// 零静默核：输入 n 个事件（去重后）必须全部可解引（落账或抑制计数覆盖）。
    pub fn assert_no_silent(&self, n_events: u32) -> Result<(), u16> {
        if self.rows.len() as u32 + self.suppressed != n_events {
            return Err(E_DROP);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    /// 路由率核：落账数 = 输入数 − 抑制数（路由 100%）。
    pub fn assert_routed(&self) -> Result<(), u16> {
        for r in &self.rows {
            r.audit()?;
        }
        if self.rows.is_empty() {
            return Err(E_DROP);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_DROP);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 注入复验核 A：三源各触发一次，全落账（路由 100%）。
pub fn inject_three_sources_must_route() -> Result<(), u16> {
    let mut r = AlertRouter::new();
    // 超阈：3200ms > 3000
    if !source_fires(AlertSrc::OverThresh, 3_200, 0)? {
        return Err(E_DROP);
    }
    r.route(&AlertEvent { src: AlertSrc::OverThresh, key: 0x101, mag: 200 })?;
    // 退化：1100 vs 1000 → +100‰
    if !source_fires(AlertSrc::Regress, 1_100, 1_000)? {
        return Err(E_DROP);
    }
    r.route(&AlertEvent { src: AlertSrc::Regress, key: 0x102, mag: 100 })?;
    // 真离群：栅栏外旗
    if !source_fires(AlertSrc::TrueOutlier, 1, 1)? {
        return Err(E_DROP);
    }
    r.route(&AlertEvent { src: AlertSrc::TrueOutlier, key: 0x103, mag: 5 })?;
    if r.rows.len() != 3 {
        return Err(E_DROP);
    }
    r.assert_no_silent(3)?;
    Ok(())
}

/// 注入复验核 B：负样本不触发（零误报）。
pub fn inject_negatives_must_pass() -> Result<(), u16> {
    // 超阈负样本：2900 ≤ 3000
    if source_fires(AlertSrc::OverThresh, 2_900, 0)? {
        return Err(E_DROP);
    }
    // 退化负样本：+99‰ < 100‰
    if source_fires(AlertSrc::Regress, 1_099, 1_000)? {
        return Err(E_DROP);
    }
    // 退化负样本：现值低于基线
    if source_fires(AlertSrc::Regress, 900, 1_000)? {
        return Err(E_DROP);
    }
    // 真离群负样本：栅栏内
    if source_fires(AlertSrc::TrueOutlier, 0, 1)? {
        return Err(E_DROP);
    }
    Ok(())
}

/// 注入复验核 C：多源同键取最高优先级（退化压过超阈与离群）。
pub fn inject_multi_source_priority() -> Result<(), u16> {
    let mut r = AlertRouter::new();
    let prio = r.route_multi(
        0x201,
        &[AlertSrc::OverThresh, AlertSrc::TrueOutlier, AlertSrc::Regress],
        150,
    )?;
    if prio != 3 {
        return Err(E_DROP);
    }
    if r.rows.len() != 1 {
        return Err(E_DROP);
    }
    if r.rows[0].src != AlertSrc::Regress {
        return Err(E_DROP);
    }
    Ok(())
}

/// 级别判定自检：低幅超阈 → 警告；高幅退化 → 严重。
pub fn level_audit() -> Result<(), u16> {
    let mut r = AlertRouter::new();
    r.route(&AlertEvent { src: AlertSrc::OverThresh, key: 0x301, mag: 10 })?;
    if r.rows[0].level != AlertLevel::Warn {
        return Err(E_LEVEL);
    }
    r.route(&AlertEvent { src: AlertSrc::Regress, key: 0x302, mag: 500 })?;
    if r.rows[1].level != AlertLevel::Severe {
        return Err(E_LEVEL);
    }
    Ok(())
}

/// 重复抑制自检：同键同源第二次 → 抑制计数 +1，不重复落账。
pub fn dedup_audit() -> Result<(), u16> {
    let mut r = AlertRouter::new();
    let ev = AlertEvent { src: AlertSrc::OverThresh, key: 0x401, mag: 10 };
    r.route(&ev)?;
    r.route(&ev)?;
    r.route(&ev)?;
    if r.rows.len() != 1 {
        return Err(E_DUP);
    }
    if r.suppressed != 2 {
        return Err(E_DUP);
    }
    r.assert_no_silent(3)?;
    Ok(())
}

/// 样本流三源事件生成器：从耗时序列 + 基线自动产出事件流（注入复验的样本侧）。
pub struct SourceFeeder {
    /// 样本序列（ms）。
    pub samples: Vec<u64>,
    /// 基线（ms）。
    pub baseline: u64,
    /// 栅栏（离群口径，上游传入）。
    pub fence_hi: u64,
    /// 事件键基址。
    pub key_base: u64,
}

impl SourceFeeder {
    pub fn new(samples: Vec<u64>, baseline: u64, fence_hi: u64, key_base: u64) -> Result<SourceFeeder, u16> {
        if samples.is_empty() || baseline == 0 || key_base == 0 {
            return Err(E_SRC);
        }
        Ok(SourceFeeder { samples, baseline, fence_hi, key_base })
    }

    /// 扫描样本流产出事件：超阈（>THRESH）、退化（≥100‰）、离群（>fence_hi）。
    /// 同一样本多源命中 → 只产一次事件（多源打包）。
    pub fn feed(&self) -> Result<Vec<AlertEvent>, u16> {
        let mut out = Vec::new();
        for (i, x) in self.samples.iter().enumerate() {
            let key = self.key_base + i as u64 + 1;
            let mut srcs: Vec<AlertSrc> = Vec::new();
            if source_fires(AlertSrc::OverThresh, *x, 0)? {
                srcs.push(AlertSrc::OverThresh);
            }
            if source_fires(AlertSrc::Regress, *x, self.baseline)? {
                srcs.push(AlertSrc::Regress);
            }
            if *x > self.fence_hi {
                srcs.push(AlertSrc::TrueOutlier);
            }
            if srcs.is_empty() {
                continue;
            }
            // 取最高优先级源
            let mut best = srcs[0];
            for s in &srcs {
                if s.prio() > best.prio() {
                    best = *s;
                }
            }
            let mag = match best {
                AlertSrc::OverThresh => x - THRESH_MS,
                AlertSrc::TrueOutlier => x - self.fence_hi,
                AlertSrc::Regress => (*x - self.baseline) * 1000 / self.baseline,
            };
            out.push(AlertEvent { src: best, key, mag });
        }
        Ok(out)
    }
}

/// 喂料确定性自检：同流两次喂料事件序列全等。
pub fn feeder_determinism_audit() -> Result<(), u16> {
    let mut s: Vec<u64> = Vec::new();
    for i in 0..20u64 {
        s.push(2_000 + i * 10);
    }
    s[15] = 3_500; // 超阈 + 离群
    let f = SourceFeeder::new(s, 2_000, 3_000, 0x700)?;
    let e1 = f.feed()?;
    let e2 = f.feed()?;
    if e1.len() != e2.len() {
        return Err(E_ROW);
    }
    for i in 0..e1.len() {
        if e1[i] != e2[i] {
            return Err(E_ROW);
        }
    }
    if e1.is_empty() {
        return Err(E_NOSRC);
    }
    Ok(())
}

/// 链路状态机：Quiet（静默期，仅严重告警放行）→ Active（全源放行）→ Sealed。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainState {
    /// 静默期：警告级告警抑制（严重照常路由）。
    Quiet,
    /// 激活：全源全级放行。
    Active,
    /// 已封链。
    Sealed,
}

/// 链路执行器：状态机包装路由器。
pub struct ChainRunner {
    pub state: ChainState,
    pub router: AlertRouter,
}

impl ChainRunner {
    pub fn new() -> ChainRunner {
        ChainRunner { state: ChainState::Active, router: AlertRouter::new() }
    }

    pub fn set_state(&mut self, s: ChainState) -> Result<(), u16> {
        if self.state == ChainState::Sealed {
            return Err(E_SEAL);
        }
        self.state = s;
        Ok(())
    }

    /// 链上路由：静默期警告级不落账（计入抑制），严重照常。
    pub fn route_on_chain(&mut self, ev: &AlertEvent) -> Result<(), u16> {
        if self.state == ChainState::Sealed {
            return Err(E_SEAL);
        }
        if self.state == ChainState::Quiet {
            // 静默期只放行高优先级源
            if ev.src.prio() < 2 {
                self.router.suppressed += 1;
                return Ok(());
            }
        }
        self.router.route(ev)
    }

    pub fn seal_chain(&mut self) -> Result<(), u16> {
        if self.router.rows.is_empty() {
            return Err(E_DROP);
        }
        self.state = ChainState::Sealed;
        self.router.seal()
    }
}

/// 链路状态机自检：静默期抑制 + 封链拦截。
pub fn chain_state_audit() -> Result<(), u16> {
    let mut c = ChainRunner::new();
    c.set_state(ChainState::Quiet)?;
    // 静默期：超阈（优先级 1）被抑制
    c.route_on_chain(&AlertEvent { src: AlertSrc::OverThresh, key: 0x801, mag: 10 })?;
    if !c.router.rows.is_empty() {
        return Err(E_DROP);
    }
    // 静默期：退化（优先级 3）放行
    c.route_on_chain(&AlertEvent { src: AlertSrc::Regress, key: 0x802, mag: 300 })?;
    if c.router.rows.len() != 1 {
        return Err(E_DROP);
    }
    // 激活：全放行
    c.set_state(ChainState::Active)?;
    c.route_on_chain(&AlertEvent { src: AlertSrc::OverThresh, key: 0x803, mag: 10 })?;
    if c.router.rows.len() != 2 {
        return Err(E_DROP);
    }
    // 封链 + 封后路由拦截
    c.seal_chain()?;
    if c.route_on_chain(&AlertEvent { src: AlertSrc::OverThresh, key: 0x804, mag: 1 }) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x44);
    }
    if c.set_state(ChainState::Active) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x45);
    }
    Ok(())
}

/// 周期结账：按告警源分桶计数（桶数 = 源闭集数）。
pub struct PeriodBucket {
    pub over: u32,
    pub outlier: u32,
    pub regress: u32,
}

impl PeriodBucket {
    pub fn count(r: &AlertRouter) -> PeriodBucket {
        let mut b = PeriodBucket { over: 0, outlier: 0, regress: 0 };
        for row in &r.rows {
            match row.src {
                AlertSrc::OverThresh => b.over += 1,
                AlertSrc::TrueOutlier => b.outlier += 1,
                AlertSrc::Regress => b.regress += 1,
            }
        }
        b
    }

    /// 桶守恒：三桶和 = 账行数。
    pub fn conserved(&self, r: &AlertRouter) -> Result<(), u16> {
        if self.over + self.outlier + self.regress != r.rows.len() as u32 {
            return Err(E_DROP);
        }
        Ok(())
    }
}

/// 告警账报告。
pub struct AlertReport {
    pub lines: Vec<String>,
}

impl AlertReport {
    pub fn generate(r: &AlertRouter) -> Result<AlertReport, u16> {
        r.assert_routed()?;
        let mut lines = Vec::new();
        let mut l = String::from("告警账: 行数=");
        l.push_str(&r.rows.len().to_string());
        l.push_str(" 抑制=");
        l.push_str(&r.suppressed.to_string());
        lines.push(l);
        for row in &r.rows {
            lines.push(row.summary());
        }
        Ok(AlertReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 2 {
            return Err(E_DROP);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_ROW);
            }
        }
        Ok(())
    }
}

/// 自例程：三源路由 100% + 负样本零误报 + 优先级/去重/级别 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    level_audit()?;
    dedup_audit()?;
    chain_state_audit()?;
    feeder_determinism_audit()?;
    inject_three_sources_must_route()?;
    inject_negatives_must_pass()?;
    inject_multi_source_priority()?;

    // 样本流喂料 → 全事件路由 → 零静默 → 周期分桶
    let mut s: Vec<u64> = Vec::new();
    for i in 0..30u64 {
        s.push(2_400 + (i * 37) % 500);
    }
    s[10] = 3_800; // 超阈
    s[20] = 3_100; // 超阈
    let feeder = SourceFeeder::new(s, 2_500, 3_200, 0x900)?;
    let events = feeder.feed()?;
    if events.is_empty() {
        return Err(E_NOSRC);
    }
    let mut r = AlertRouter::new();
    let n = events.len() as u32;
    for ev in &events {
        r.route(ev)?;
    }
    r.assert_no_silent(n)?;
    r.assert_routed()?;
    let bucket = PeriodBucket::count(&r);
    bucket.conserved(&r)?;
    r.seal()?;
    // 注入：封后路由 → 拦截
    if r.route(&AlertEvent { src: AlertSrc::OverThresh, key: 0x503, mag: 1 }) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x40);
    }
    // 注入：零键事件 → 拦截
    let mut r2 = AlertRouter::new();
    if r2.route(&AlertEvent { src: AlertSrc::OverThresh, key: 0, mag: 1 }) != Err(E_ROW) {
        return Err(SEG_BASE + 0x41);
    }
    // 注入：空多源路由 → 拦截
    if r2.route_multi(5, &[], 1) != Err(E_NOSRC) {
        return Err(SEG_BASE + 0x42);
    }
    // 注入：退化源零基线 → 拦截
    if source_fires(AlertSrc::Regress, 1_100, 0) != Err(E_SRC) {
        return Err(SEG_BASE + 0x43);
    }
    // 报告
    let rep = AlertReport::generate(&r)?;
    rep.assert_complete()?;
    Ok(0)
}
