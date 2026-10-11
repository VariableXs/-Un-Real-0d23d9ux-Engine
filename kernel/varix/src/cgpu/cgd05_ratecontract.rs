//! cgd05_ratecontract — CGPU-F0485 帧率合同校验器（把稳定帧率从口号变合同）。
//!
//! 判据原文（施工书锚点 CGPU-F0485）：合同校验：逐帧对照条款（P95/P99/卡顿
//! 三条款的滚动窗口自动判定）；达标率输出（每场景每时段的达标率——合同履行
//! 报告）；违反预警（逼近违反的早期趋势——提前干预而非事后报告）；校验器与
//! 配方联动（不达标→预算修正建议触发——C 域集成复用）。判据：三条款、达标
//! 率、预警、联动、判据。
//!
//! 职责定位：滚动窗口三条款判定+履行报告+早期预警+预算修正建议单。与
//! cgd04_frameinterval（F0484 帧间隔统计）分工——彼是全量分布统计与分级，
//! 此是合同条款的合规判定（阈值独立写死可对拍，不 import 彼阈值——漂移由
//! 对账纪律发现）。预算修正建议只出单不执行（C 域 cgc02/cgc03 池消费）。
//! 分位判定用 0.5ms 桶计数排序（O(n+桶)，不用比较排序）。诊断码段 0x3Dxx。

use alloc::vec::Vec;

/// 合同版本（合同文本随版本冻结，改动走 ADR——版本号即冻结标识）。
pub const CONTRACT_VERSION: &str = "D05-ratecontract-v1";

/// 滚动窗口 600 帧（10s @60fps 基准；判据"滚动窗口自动判定"）。
pub const WINDOW_FRAMES: usize = 600;

/// 条款一：P95 帧间隔 ≤ 12.5ms（80fps 承诺；与 CGPU-F0006 合同基准同口径）。
pub const P95_CEIL_US: u32 = 12_500;
/// 条款二：P99 帧间隔 ≤ 25ms（两倍承诺——尾延迟红线）。
pub const P99_CEIL_US: u32 = 25_000;
/// 条款三：卡顿率（间隔 >50ms 记卡顿）≤ 0.5%（5000ppm）。
pub const JANK_CEIL_PPM: u64 = 5_000;
/// 卡顿判定线 50ms（判据原文三条款之"卡顿"）。
pub const JANK_LINE_US: u32 = 50_000;

/// 分位桶：0.5ms 一桶，覆盖 0~200ms（400 桶；超界样本归顶桶并单独计数）。
pub const BUCKET_US: u32 = 500;
pub const BUCKETS: usize = 400;

/// 预警余量线：窗口指标已用掉阈值余量的 80% 即升预警（提前干预判据）。
pub const WARN_HEADROOM_PPM: u64 = 800_000;

/// 诊断码（0x3Dxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x3D01 窗口未满不做合同判定（样本不足显性——拒绝给结论）。
    RcWindowNotFull,
    /// 0x3D02 条款违反已立案（履行报告带立案计数）。
    RcClauseBroken,
    /// 0x3D03 样本非法（0µs 或超 200ms 量程——量程外显性不静默截断）。
    RcSampleInvalid,
    /// 0x3D04 预警已升起（提前干预信号，消费方必须响应）。
    RcWarnRisen,
    /// 0x3D05 报告序断裂（时段序不单调——履行账完整性破坏）。
    RcReportBroken,
    /// 0x3D06 申请判定未带场景标签（达标率必须按场景×时段输出）。
    RcSceneUnlabeled,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::RcWindowNotFull => "滚动窗口未满：样本不足不下合同结论（宁缺毋滥）",
            XcCode::RcClauseBroken => "合同条款违反：三条款至少一条失守，已立案并出预算修正建议",
            XcCode::RcSampleInvalid => "帧间隔样本非法：0µs 或超 200ms 量程，显性拒收不静默截断",
            XcCode::RcWarnRisen => "违反预警：指标逼近阈值（余量已耗 80%），提前干预窗口",
            XcCode::RcReportBroken => "履行报告时段序断裂：账本必须按时段单调",
            XcCode::RcSceneUnlabeled => "判定请求未带场景标签：达标率按场景×时段双维输出",
        }
    }
}

/// 场景标签闭集（对齐 CGPU-F0006 三档基准 + 兜底）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 基准 A：30 网页静态。
    BenchA,
    /// 基准 B：30 网页 + 4 路 4K 动图。
    BenchB,
    /// 基准 C：30 网页 + 8 路 4K 动图 + 视频墙。
    BenchC,
    /// 非基准场景（自由负载）。
    Free,
}

impl Scene {
    pub const fn zh(self) -> &'static str {
        match self {
            Scene::BenchA => "基准A-30网页静态",
            Scene::BenchB => "基准B-30网页+4路4K动图",
            Scene::BenchC => "基准C-30网页+8路4K动图+视频墙",
            Scene::Free => "自由负载",
        }
    }
}

/// 滚动窗口（桶计数分位器——样本进桶不存原值，O(1) 入窗/O(n+桶) 分位）。
pub struct RollingWin {
    /// 帧间隔桶计数。
    buckets: [u32; BUCKETS],
    /// 量程外样本计数（>200ms；显性单独计）。
    over_range: u32,
    /// 卡顿样本计数（>50ms）。
    jank: u32,
    /// 窗内样本总数。
    n: u32,
    /// 样本和（µs——均值用）。
    sum_us: u64,
    /// 样本环（覆盖序：满 600 后逐个顶替旧桶——按入窗序退桶）。
    ring: [u32; WINDOW_FRAMES],
    /// 环写指针。
    head: usize,
    /// 窗口是否已滚动满一轮（顶替开始后旧值退桶）。
    wrapped: bool,
}

impl RollingWin {
    pub const fn new() -> Self {
        RollingWin {
            buckets: [0; BUCKETS],
            over_range: 0,
            jank: 0,
            n: 0,
            sum_us: 0,
            ring: [0; WINDOW_FRAMES],
            head: 0,
            wrapped: false,
        }
    }

    /// 样本入窗（µs；非法样本显性拒）。
    pub fn push(&mut self, gap_us: u32) -> Result<(), XcCode> {
        if gap_us == 0 || gap_us > 200_000 {
            return Err(XcCode::RcSampleInvalid);
        }
        // 退旧桶（窗口满后顶替）
        if self.wrapped {
            let old = self.ring[self.head];
            let bi = (old / BUCKET_US) as usize;
            let bi = if bi >= BUCKETS { BUCKETS - 1 } else { bi };
            self.buckets[bi] = self.buckets[bi].saturating_sub(1);
            self.n = self.n.saturating_sub(1);
            self.sum_us = self.sum_us.saturating_sub(old as u64);
            if old > JANK_LINE_US {
                self.jank = self.jank.saturating_sub(1);
            }
            if old as usize / BUCKET_US as usize >= BUCKETS {
                self.over_range = self.over_range.saturating_sub(1);
            }
        }
        // 进新桶
        let bi = (gap_us / BUCKET_US) as usize;
        if bi >= BUCKETS {
            self.over_range = self.over_range.saturating_add(1);
            self.buckets[BUCKETS - 1] += 1;
        } else {
            self.buckets[bi] += 1;
        }
        if gap_us > JANK_LINE_US {
            self.jank = self.jank.saturating_add(1);
        }
        self.ring[self.head] = gap_us;
        self.head = (self.head + 1) % WINDOW_FRAMES;
        if self.head == 0 {
            self.wrapped = true;
        }
        self.n = self.n.saturating_add(1);
        self.sum_us = self.sum_us.saturating_add(gap_us as u64);
        Ok(())
    }

    /// 窗口是否满（未满拒绝合同判定——RcWindowNotFull）。
    pub const fn full(&self) -> bool {
        self.n as usize >= WINDOW_FRAMES
    }

    /// 桶计数分位（ppm 定位；线性扫桶）。
    fn percentile_us(&self, ppm: u64) -> u32 {
        let target = (self.n as u64).saturating_mul(ppm) / 1_000_000;
        let target = target.max(1);
        let mut acc = 0u64;
        for (i, &c) in self.buckets.iter().enumerate() {
            acc += c as u64;
            if acc >= target {
                return ((i as u32) * BUCKET_US).min(200_000);
            }
        }
        200_000
    }

    /// P95（µs；桶上沿即代表值——0.5ms 粒度，判定留 0.5ms 天然余量）。
    pub fn p95_us(&self) -> u32 {
        self.percentile_us(950_000)
    }

    /// P99（µs）。
    pub fn p99_us(&self) -> u32 {
        self.percentile_us(990_000)
    }

    /// 卡顿率（ppm）。
    pub fn jank_ppm(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        (self.jank as u64) * 1_000_000 / self.n as u64
    }

    /// 均值（µs）。
    pub fn mean_us(&self) -> u64 {
        if self.n == 0 {
            0
        } else {
            self.sum_us / self.n as u64
        }
    }

    /// 量程外样本数（>0 即有静默截断风险——调用方应关注）。
    pub const fn over_range(&self) -> u32 {
        self.over_range
    }

    /// 首半窗 vs 后半窗卡顿率对比（预警趋势项：后半显著更差即趋势升）。
    /// 环语义：ring[head-1] 最新、ring[head] 最旧；年龄 age=(head-1-idx) 环序，
    /// age<half 为新半窗、其余为旧半窗——两半窗样本量恒等（各 300）。
    pub fn jank_trend(&self) -> Trend {
        if !self.wrapped {
            return Trend::Flat;
        }
        let half = WINDOW_FRAMES / 2;
        let (mut older_jank, mut newer_jank) = (0u32, 0u32);
        for (idx, &v) in self.ring.iter().enumerate() {
            if v <= JANK_LINE_US {
                continue;
            }
            let age = (self.head + WINDOW_FRAMES - 1 - idx) % WINDOW_FRAMES;
            if age < half {
                newer_jank += 1;
            } else {
                older_jank += 1;
            }
        }
        if newer_jank > older_jank * 2 && newer_jank >= 3 {
            Trend::Rising
        } else if older_jank > newer_jank * 2 && older_jank >= 3 {
            Trend::Falling
        } else {
            Trend::Flat
        }
    }
}

/// 趋势三态（预警用：Rising 即提前干预信号之一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trend {
    Rising,
    Flat,
    Falling,
}

/// 三条款判定行（单窗口结论）。
#[derive(Clone, Copy, Debug)]
pub struct ClauseRow {
    /// P95 实测（µs）。
    pub p95_us: u32,
    /// P99 实测（µs）。
    pub p99_us: u32,
    /// 卡顿率（ppm）。
    pub jank_ppm: u64,
    /// 三条款布尔。
    pub ok_p95: bool,
    pub ok_p99: bool,
    pub ok_jank: bool,
}

impl ClauseRow {
    /// 全条款达标。
    pub const fn all_ok(&self) -> bool {
        self.ok_p95 && self.ok_p99 && self.ok_jank
    }

    /// 达标条款数（0~3）。
    pub const fn ok_count(&self) -> u8 {
        (self.ok_p95 as u8) + (self.ok_p99 as u8) + (self.ok_jank as u8)
    }

    /// 达标率（ppm：达标条款数/3）。
    pub const fn ok_rate_ppm(&self) -> u64 {
        (self.ok_count() as u64) * 1_000_000 / 3
    }

    /// 最紧条款余量（已用 ppm——预警判据输入：≥WARN_HEADROOM_PPM 即预警）。
    pub fn tightest_used_ppm(&self) -> u64 {
        let used_p95 = if self.p95_us as u64 >= P95_CEIL_US as u64 {
            1_000_000
        } else {
            (self.p95_us as u64) * 1_000_000 / P95_CEIL_US as u64
        };
        let used_p99 = if self.p99_us as u64 >= P99_CEIL_US as u64 {
            1_000_000
        } else {
            (self.p99_us as u64) * 1_000_000 / P99_CEIL_US as u64
        };
        // 卡顿率直接就是 ppm 口径
        used_p95.max(used_p99).max(self.jank_ppm)
    }
}

/// 预算修正建议单（判据"不达标→预算修正建议触发——C 域集成复用"）。
/// 建议只出单不执行：C 域池（cgc02/cgc03）消费后按其水位裁量。
#[derive(Clone, Copy, Debug)]
pub struct BudgetCorrection {
    /// 建议对象池（对齐 cgc02_budgetmodel::PoolKind 四池 ordinal：
    /// 0=呈现保底 1=交互 2=表面 3=机动——按违反条款映射）。
    pub pool_ordinal: usize,
    /// 建议增量（ppm，正=加预算，负=收缩）。
    pub delta_ppm: i64,
    /// 归因条款（0=P95 1=P99 2=卡顿）。
    pub cause_clause: u8,
}

impl BudgetCorrection {
    /// 按违反条款出建议（保守策略：P95 失守→加交互池；P99 失守→收缩表面池；
    /// 卡顿失守→收缩机动池补给交互——方向单值，不静默）。
    pub fn derive(row: &ClauseRow) -> Vec<BudgetCorrection> {
        let mut out = Vec::new();
        if !row.ok_p95 {
            out.push(BudgetCorrection { pool_ordinal: 1, delta_ppm: 30_000, cause_clause: 0 });
        }
        if !row.ok_p99 {
            out.push(BudgetCorrection { pool_ordinal: 2, delta_ppm: -30_000, cause_clause: 1 });
        }
        if !row.ok_jank {
            out.push(BudgetCorrection { pool_ordinal: 3, delta_ppm: -20_000, cause_clause: 2 });
        }
        out
    }
}

/// 合同履行报告行（判据"每场景每时段的达标率"）。
#[derive(Clone, Debug)]
pub struct PerfRow {
    /// 时段序（单调；断裂=RcReportBroken）。
    pub slot: u64,
    /// 场景标签。
    pub scene: Scene,
    /// 三条款判定。
    pub clause: ClauseRow,
    /// 预警是否升起。
    pub warned: bool,
    /// 预算修正建议（空=无需修正）。
    pub corrections: Vec<BudgetCorrection>,
}

/// 合同校验器本体。
pub struct RateContract {
    /// 滚动窗口。
    win: RollingWin,
    /// 当前场景标签（判定按场景×时段双维）。
    scene: Scene,
    /// 时段发生器。
    slot: u64,
    /// 履行账。
    ledger: Vec<PerfRow>,
    /// 立案计数。
    pub cases: u32,
    /// 预警计数。
    pub warns: u32,
}

impl RateContract {
    pub fn new(scene: Scene) -> Self {
        RateContract {
            win: RollingWin::new(),
            scene,
            slot: 0,
            ledger: Vec::new(),
            cases: 0,
            warns: 0,
        }
    }

    /// 逐帧喂样本（µs）。
    pub fn feed(&mut self, gap_us: u32) -> Result<(), XcCode> {
        self.win.push(gap_us)
    }

    /// 切换场景（清窗重计——合同达标率按场景隔离，跨场景混算会撒谎）。
    pub fn switch_scene(&mut self, scene: Scene) {
        if scene != self.scene {
            self.win = RollingWin::new();
            self.scene = scene;
        }
    }

    pub const fn current_scene(&self) -> Scene {
        self.scene
    }

    /// 窗口判定（判据主路径：三条款滚动窗口自动判定 → 履行报告行）。
    /// 返回报告行副本；窗口未满显性拒。
    pub fn judge(&mut self) -> Result<PerfRow, XcCode> {
        if !self.win.full() {
            return Err(XcCode::RcWindowNotFull);
        }
        let row = ClauseRow {
            p95_us: self.win.p95_us(),
            p99_us: self.win.p99_us(),
            jank_ppm: self.win.jank_ppm(),
            ok_p95: self.win.p95_us() <= P95_CEIL_US,
            ok_p99: self.win.p99_us() <= P99_CEIL_US,
            ok_jank: self.win.jank_ppm() <= JANK_CEIL_PPM,
        };
        // 违反立案
        if !row.all_ok() {
            self.cases = self.cases.saturating_add(1);
        }
        // 预警双源：余量耗用 ≥80%，或卡顿趋势 Rising
        let headroom = row.tightest_used_ppm();
        let warn = headroom >= WARN_HEADROOM_PPM || self.win.jank_trend() == Trend::Rising;
        if warn && row.all_ok() {
            // 未违反但预警：单独计数（违反已在 cases 里计过）
            self.warns = self.warns.saturating_add(1);
        }
        self.slot = self.slot.saturating_add(1);
        let perf = PerfRow {
            slot: self.slot,
            scene: self.scene,
            clause: row,
            warned: warn,
            corrections: if row.all_ok() { Vec::new() } else { BudgetCorrection::derive(&row) },
        };
        self.ledger.push(perf.clone());
        if warn && !row.all_ok() {
            self.warns = self.warns.saturating_add(1);
            return Err(XcCode::RcWarnRisen); // 预警+违反双信号：调用方先看预警
        }
        if !row.all_ok() {
            return Err(XcCode::RcClauseBroken);
        }
        if warn {
            return Err(XcCode::RcWarnRisen);
        }
        Ok(perf)
    }

    /// 履行账全量。
    pub fn ledger(&self) -> &[PerfRow] {
        &self.ledger
    }

    /// 履行账时段序连续性（断裂=RcReportBroken）。立案写账用 &mut self。
    pub fn ledger_sequential(&mut self) -> Result<(), XcCode> {
        let mut prev = 0u64;
        for r in &self.ledger {
            if r.slot <= prev {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::RcReportBroken);
            }
            prev = r.slot;
        }
        Ok(())
    }

    /// 全周期达标率（ppm：全条款达标窗口数/总窗口数）。
    pub fn overall_ok_ppm(&self) -> u64 {
        if self.ledger.is_empty() {
            return 0;
        }
        let ok = self.ledger.iter().filter(|r| r.clause.all_ok()).count();
        (ok as u64) * 1_000_000 / self.ledger.len() as u64
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "合同{} 场景{} 时段{} 窗口{}/{} 全周期达标{}ppm 立案{} 预警{}",
            CONTRACT_VERSION,
            self.scene.zh(),
            self.slot,
            self.win.n,
            WINDOW_FRAMES,
            self.overall_ok_ppm(),
            self.cases,
            self.warns,
        )
    }
}
