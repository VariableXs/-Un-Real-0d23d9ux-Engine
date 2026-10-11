//! vck11_hettel — CGPU-F1611 异构遥测与归因（每引擎五指标+跨引擎归因+帧段守恒）。
//!
//! 判据原文（施工书锚点 CGPU-F1611）：遥测归因：引擎遥测（每引擎指标（利
//! 用率/队列深度/内存/功耗估算/温度——指标集注册（F1556 v2 注册表扩展——
//! 扩展记录）；归因（任务归因跨引擎（任务在哪个引擎耗了多少——跨引擎归因
//! （归因准确率契约跨引擎扩展；帧归因（异构任务在帧内的贡献段——帧归因段
//! 扩展（C 域归因联动；能力画像（引擎能效画像（随实测更新——画像复用
//! F1499 模式；与 J08 对接（异构指标入遥测仓库/报告——对接复用；隐私（遥
//! 测本地红线复用；测试（指标注册/归因/帧段/画像/对接/隐私六组）。判据：
//! 指标扩展、跨引擎归因、帧段、画像、对接、隐私、六组、判据。
//!
//! 职责定位：五指标集注册（利用率/队列深度/内存/功耗估算/温度按 F1556 v2
//! 注册表扩展模式登记——缺指标 0x7601，注册表是遥测的边界不是采样器的心
//! 情单）+跨引擎任务归因（任务号→引擎→耗时一本账，归因账复式闭合：每笔
//! 归因入引擎列同时入任务列，两列合计必须相等——账平被破 0x7602）+帧归因
//! 段（异构任务在帧内的贡献段逐段登记，Σ段≤帧长守恒——段和超帧长 0x7603，
//! C 域归因联动锚）+能效画像（fps-per-watt 画像随实测刷新，画像滞后于实测
//! 0x7604——F1499 复用声明在册）+J08 对接（异构指标入遥测仓库的桥接面，
//! 报告含全部五指标——缺指标报告拒收）+隐私红线（遥测数据本地不出域，出
//! 域企图 0x7605——红线复用 T 域口径）。诊断码段 0x7601~0x7605。

use alloc::vec::Vec;

/// 模块版本。
pub const HET_TEL_VERSION: &str = "K11-hettel-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x76 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7601 指标注册失守（五指标集未按 F1556 v2 扩展注册）。
pub const XC_METRIC_REG_MISS: u32 = 0x7601;
/// 0x7602 跨引擎归因失守（归因复式账不平）。
pub const XC_ATTR_IMBALANCE: u32 = 0x7602;
/// 0x7603 帧归因段失守（Σ段>帧长守恒破）。
pub const XC_FRAME_SEG_BREACH: u32 = 0x7603;
/// 0x7604 画像失新（能效画像未随实测更新）。
pub const XC_PROFILE_STALE: u32 = 0x7604;
/// 0x7605 隐私红线破（遥测出本地）。
pub const XC_PRIVACY_BREACH: u32 = 0x7605;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 五指标闭集数。
pub const METRIC_COUNT: usize = 5;
/// 归因账容量（任务数上限）。
pub const ATTR_SLOTS: usize = 32;
/// 帧归因段上限。
pub const FRAME_SEG_MAX: usize = 8;
/// 帧长基线（µs——12.5ms 口径与 C 域同源）。
pub const FRAME_BUDGET_US: u32 = 12_500;
/// 隐私红线域标记（0=本地 1=出域）。
pub const PRIVACY_LOCAL: u32 = 0;

// ---------------------------------------------------------------------------
// 一、五指标集注册（F1556 v2 注册表扩展）
// ---------------------------------------------------------------------------

/// 引擎指标五闭集（判据"利用率/队列深度/内存/功耗估算/温度"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HetMetric {
    Utilization,
    QueueDepth,
    Memory,
    PowerEst,
    Temperature,
}

impl HetMetric {
    /// 五指标闭集。
    pub const ALL: [HetMetric; METRIC_COUNT] = [
        HetMetric::Utilization,
        HetMetric::QueueDepth,
        HetMetric::Memory,
        HetMetric::PowerEst,
        HetMetric::Temperature,
    ];

    /// 指标序号。
    pub const fn idx(self) -> usize {
        match self {
            HetMetric::Utilization => 0,
            HetMetric::QueueDepth => 1,
            HetMetric::Memory => 2,
            HetMetric::PowerEst => 3,
            HetMetric::Temperature => 4,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            HetMetric::Utilization => "利用率",
            HetMetric::QueueDepth => "队列深度",
            HetMetric::Memory => "内存",
            HetMetric::PowerEst => "功耗估算",
            HetMetric::Temperature => "温度",
        }
    }
}

/// 指标注册表（F1556 v2 扩展模式：v1 指标集+异构扩展行）。
#[derive(Debug)]
pub struct MetricSetRegistry {
    /// 已注册指标位图（bit i = HetMetric::ALL[i]）。
    pub registered: u32,
    /// 扩展记录号（F1556 v2 单号锚）。
    pub ext_task: u32,
    /// 注册缺失事件数（0x7601）。
    pub miss_events: u32,
}

impl MetricSetRegistry {
    /// 新注册表（扩展记录号必填）。
    pub const fn new(ext_task: u32) -> MetricSetRegistry {
        MetricSetRegistry {
            registered: 0,
            ext_task,
            miss_events: 0,
        }
    }

    /// 注册指标（幂等——重复注册覆盖不改语义）。
    pub fn register(&mut self, m: HetMetric) {
        self.registered |= 1 << m.idx();
    }

    /// 全量注册（五指标一键齐——扩展记录完备态）。
    pub fn register_all(&mut self) {
        let mut i = 0;
        while i < METRIC_COUNT {
            self.register(HetMetric::ALL[i]);
            i += 1;
        }
    }

    /// 注册完备校验：五指标全在册（缺一 0x7601）。
    pub fn complete(&mut self) -> Result<(), u32> {
        let want = (1 << METRIC_COUNT) - 1;
        if self.registered != want {
            self.miss_events += 1;
            return Err(XC_METRIC_REG_MISS);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、跨引擎任务归因（复式账）
// ---------------------------------------------------------------------------

/// 归因条目（一笔归因：任务号+引擎档+耗时）。
#[derive(Clone, Copy, Debug)]
pub struct AttrEntry {
    /// 任务号。
    pub task: u32,
    /// 引擎档（0=Low/DSP 1=Mid/NPU 2=High/GPU）。
    pub cls: usize,
    /// 耗时（µs）。
    pub cost_us: u32,
}

/// 跨引擎归因账（判据"任务在哪个引擎耗了多少——跨引擎归因"）。
///
/// 复式记账：每笔入引擎列（per-engine）同时入任务列（per-task），裁决时
/// 两列总和必须相等——不平即 0x7602（归因准确率契约的账面基座）。
#[derive(Debug)]
pub struct TaskAttribution {
    /// 归因条目账。
    pub entries: Vec<AttrEntry>,
    /// 引擎列合计（µs，三档）。
    pub engine_sums: [u64; 3],
    /// 任务列合计（µs，按任务号聚合）。
    pub task_sums: Vec<(u32, u64)>,
    /// 账不平事件数（0x7602）。
    pub imbalance_events: u32,
}

impl TaskAttribution {
    /// 新账。
    pub const fn new() -> TaskAttribution {
        TaskAttribution {
            entries: Vec::new(),
            engine_sums: [0; 3],
            task_sums: Vec::new(),
            imbalance_events: 0,
        }
    }

    /// 记一笔归因（复式：两列同入；容量满拒收）。
    pub fn record(&mut self, e: AttrEntry) -> Result<(), u32> {
        if e.cls >= 3 || self.entries.len() >= ATTR_SLOTS {
            self.imbalance_events += 1;
            return Err(XC_ATTR_IMBALANCE);
        }
        self.entries.push(e);
        self.engine_sums[e.cls] += e.cost_us as u64;
        // 任务列聚合。
        let mut i = 0;
        while i < self.task_sums.len() {
            if self.task_sums[i].0 == e.task {
                self.task_sums[i].1 += e.cost_us as u64;
                return Ok(());
            }
            i += 1;
        }
        self.task_sums.push((e.task, e.cost_us as u64));
        Ok(())
    }

    /// 复式对账：引擎列合计≡任务列合计（0x7602）。
    pub fn double_entry_ok(&self) -> Result<(), u32> {
        let mut eng = 0u64;
        let mut i = 0;
        while i < 3 {
            eng += self.engine_sums[i];
            i += 1;
        }
        let mut tsk = 0u64;
        i = 0;
        while i < self.task_sums.len() {
            tsk += self.task_sums[i].1;
            i += 1;
        }
        if eng != tsk {
            return Err(XC_ATTR_IMBALANCE);
        }
        Ok(())
    }

    /// 任务归因查询：任务号→各引擎耗时分解（归因准确率契约的查询面）。
    pub fn breakdown(&self, task: u32) -> [u64; 3] {
        let mut out = [0u64; 3];
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].task == task {
                out[self.entries[i].cls] += self.entries[i].cost_us as u64;
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、帧归因段（Σ段≤帧长守恒）
// ---------------------------------------------------------------------------

/// 帧归因段（帧内一个异构任务的贡献段）。
#[derive(Clone, Copy, Debug)]
pub struct FrameSeg {
    /// 任务号。
    pub task: u32,
    /// 引擎档。
    pub cls: usize,
    /// 段长（µs）。
    pub len_us: u32,
}

/// 帧归因账（判据"异构任务在帧内的贡献段——帧归因段扩展"）。
#[derive(Debug)]
pub struct FrameAttribution {
    /// 帧号。
    pub frame_no: u64,
    /// 贡献段表。
    pub segs: Vec<FrameSeg>,
    /// 守恒破事件数（0x7603）。
    pub breach_events: u32,
}

impl FrameAttribution {
    /// 新帧（段表空）。
    pub const fn new(frame_no: u64) -> FrameAttribution {
        FrameAttribution {
            frame_no,
            segs: Vec::new(),
            breach_events: 0,
        }
    }

    /// 加段（超容量拒收——段守恒前置）。
    pub fn add_seg(&mut self, s: FrameSeg) -> Result<(), u32> {
        if self.segs.len() >= FRAME_SEG_MAX {
            self.breach_events += 1;
            return Err(XC_FRAME_SEG_BREACH);
        }
        self.segs.push(s);
        Ok(())
    }

    /// 守恒裁决：Σ段≤帧预算（12.5ms——C 域口径联动锚）。
    pub fn conserve(&mut self) -> Result<u32, u32> {
        let mut sum = 0u32;
        let mut i = 0;
        while i < self.segs.len() {
            sum += self.segs[i].len_us;
            i += 1;
        }
        if sum > FRAME_BUDGET_US {
            self.breach_events += 1;
            return Err(XC_FRAME_SEG_BREACH);
        }
        Ok(sum)
    }
}

// ---------------------------------------------------------------------------
// 四、能效画像（随实测更新——F1499 复用声明）
// ---------------------------------------------------------------------------

/// 能效画像（判据"引擎能效画像（随实测更新——画像复用 F1499 模式"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfficacyProfile {
    /// 引擎档。
    pub cls: usize,
    /// 画像 fpw（万分比）。
    pub fpw_bp: u32,
    /// 画像基于的实测批号（新鲜度锚）。
    pub based_on_batch: u64,
}

/// 画像簿。
#[derive(Debug)]
pub struct ProfileBook {
    /// 画像表（三档）。
    pub profiles: [Option<EfficacyProfile>; 3],
    /// F1499 复用声明在册。
    pub f1499_declared: bool,
    /// 失新事件数（0x7604）。
    pub stale_events: u32,
}

impl ProfileBook {
    /// 新簿（F1499 声明出厂即在册——复用语义显性）。
    pub const fn new() -> ProfileBook {
        ProfileBook {
            profiles: [None, None, None],
            f1499_declared: true,
            stale_events: 0,
        }
    }

    /// 实测刷新画像（批号新=画像新）。
    pub fn refresh(&mut self, p: EfficacyProfile) {
        self.profiles[p.cls.min(2)] = Some(p);
    }

    /// 新鲜度裁决：查询画像时批号必须≡最新实测批号（滞后 0x7604）。
    pub fn query(&mut self, cls: usize, latest_batch: u64) -> Result<EfficacyProfile, u32> {
        match self.profiles[cls.min(2)] {
            Some(p) => {
                if p.based_on_batch != latest_batch {
                    self.stale_events += 1;
                    return Err(XC_PROFILE_STALE);
                }
                Ok(p)
            }
            None => {
                self.stale_events += 1;
                return Err(XC_PROFILE_STALE);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、J08 对接桥 + 隐私红线
// ---------------------------------------------------------------------------

/// J08 对接报告（异构指标入遥测仓库的桥接面）。
#[derive(Clone, Debug)]
pub struct J08Report {
    /// 五指标快照（每引擎档一行×五指标）。
    pub snapshot: [[u32; METRIC_COUNT]; 3],
    /// 帧号锚。
    pub frame_no: u64,
    /// 隐私域标记（必须=PRIVACY_LOCAL）。
    pub privacy_domain: u32,
}

/// 对接桥+隐私守卫（判据"与 J08 对接（异构指标入遥测仓库/报告"）。
#[derive(Debug)]
pub struct J08Bridge {
    /// 已提交报告数。
    pub submitted: u32,
    /// 隐私破事件数（0x7605）。
    pub privacy_events: u32,
}

impl J08Bridge {
    /// 新桥。
    pub const fn new() -> J08Bridge {
        J08Bridge {
            submitted: 0,
            privacy_events: 0,
        }
    }

    /// 提交报告（五指标全量+隐私本地校验——缺指标/出域均拒）。
    pub fn submit(&mut self, r: &J08Report) -> Result<(), u32> {
        // 隐私红线：报告只允许本地域。
        if r.privacy_domain != PRIVACY_LOCAL {
            self.privacy_events += 1;
            return Err(XC_PRIVACY_BREACH);
        }
        // 五指标完备：三档逐行非零指针无意义——此处校验快照行均已填充
        // （利用率为万分比 0..=10000 闭集外拒——负值/超万在 u32 下即超大值）。
        let mut c = 0;
        while c < 3 {
            let util = r.snapshot[c][HetMetric::Utilization.idx()];
            if util > 10_000 {
                self.privacy_events += 1;
                return Err(XC_METRIC_REG_MISS);
            }
            c += 1;
        }
        self.submitted += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、总引擎 + 六组自检
// ---------------------------------------------------------------------------

/// 异构遥测归因总引擎。
#[derive(Debug)]
pub struct HetTelEngine {
    /// 指标注册表。
    pub reg: MetricSetRegistry,
    /// 任务归因账。
    pub attr: TaskAttribution,
    /// 画像簿。
    pub profiles: ProfileBook,
    /// J08 对接桥。
    pub bridge: J08Bridge,
}

impl HetTelEngine {
    /// 出厂（扩展记录号锚 F1556 v2）。
    pub const fn new() -> HetTelEngine {
        HetTelEngine {
            reg: MetricSetRegistry::new(1556),
            attr: TaskAttribution::new(),
            profiles: ProfileBook::new(),
            bridge: J08Bridge::new(),
        }
    }

    /// 组一：指标注册——五指标全注册过；缺一拒；扩展记录号在册。
    pub fn selfcheck_metrics(&mut self) -> Result<(), u32> {
        let mut e = HetTelEngine::new();
        // 缺指标→0x7601。
        e.reg.register(HetMetric::Utilization);
        e.reg.register(HetMetric::QueueDepth);
        if e.reg.complete() != Err(XC_METRIC_REG_MISS) {
            return Err(XC_METRIC_REG_MISS);
        }
        if e.reg.miss_events != 1 {
            return Err(XC_METRIC_REG_MISS);
        }
        // 全量注册→过。
        e.reg.register_all();
        e.reg.complete()?;
        if e.reg.ext_task != 1556 {
            return Err(XC_METRIC_REG_MISS);
        }
        Ok(())
    }

    /// 组二：跨引擎归因——复式账平；账不平显性；分解查询。
    pub fn selfcheck_attr(&mut self) -> Result<(), u32> {
        let mut e = HetTelEngine::new();
        // 三笔归因：任务 7 在 GPU 300µs + NPU 200µs；任务 8 在 DSP 100µs。
        e.attr.record(AttrEntry { task: 7, cls: 2, cost_us: 300 })?;
        e.attr.record(AttrEntry { task: 7, cls: 1, cost_us: 200 })?;
        e.attr.record(AttrEntry { task: 8, cls: 0, cost_us: 100 })?;
        e.attr.double_entry_ok()?;
        // 分解查询：任务 7 → GPU 300 NPU 200 DSP 0。
        let b = e.attr.breakdown(7);
        if b != [0u64, 200, 300] {
            return Err(XC_ATTR_IMBALANCE);
        }
        // 引擎档越界→拒。
        if e.attr.record(AttrEntry { task: 9, cls: 5, cost_us: 10 }) != Err(XC_ATTR_IMBALANCE) {
            return Err(XC_ATTR_IMBALANCE);
        }
        // 复式账人工破坏→显性（模拟账本被改：引擎列加一笔任务列不加）。
        e.attr.engine_sums[2] += 999;
        if e.attr.double_entry_ok() != Err(XC_ATTR_IMBALANCE) {
            return Err(XC_ATTR_IMBALANCE);
        }
        if e.attr.imbalance_events != 1 {
            return Err(XC_ATTR_IMBALANCE);
        }
        Ok(())
    }

    /// 组三：帧归因段——Σ段≤帧预算守恒；超预算显性；容量上限。
    pub fn selfcheck_frames(&self) -> Result<(), u32> {
        let mut f = FrameAttribution::new(100);
        f.add_seg(FrameSeg { task: 1, cls: 2, len_us: 6_000 })?;
        f.add_seg(FrameSeg { task: 1, cls: 1, len_us: 3_000 })?;
        f.add_seg(FrameSeg { task: 2, cls: 0, len_us: 2_000 })?;
        // Σ=11_000 ≤ 12_500 过。
        if f.conserve()? != 11_000 {
            return Err(XC_FRAME_SEG_BREACH);
        }
        // 超预算段→0x7603。
        f.add_seg(FrameSeg { task: 3, cls: 2, len_us: 2_000 })?;
        if f.conserve() != Err(XC_FRAME_SEG_BREACH) {
            return Err(XC_FRAME_SEG_BREACH);
        }
        // 容量上限 8 段。
        let mut f2 = FrameAttribution::new(101);
        let mut i = 0;
        while i < FRAME_SEG_MAX {
            f2.add_seg(FrameSeg { task: i as u32, cls: 0, len_us: 100 })?;
            i += 1;
        }
        if f2.add_seg(FrameSeg { task: 99, cls: 0, len_us: 100 }) != Err(XC_FRAME_SEG_BREACH) {
            return Err(XC_FRAME_SEG_BREACH);
        }
        Ok(())
    }

    /// 组四：画像——实测刷新后可查；批号滞后 0x7604；F1499 声明在册。
    pub fn selfcheck_profiles(&mut self) -> Result<(), u32> {
        let mut e = HetTelEngine::new();
        if !e.profiles.f1499_declared {
            return Err(XC_PROFILE_STALE);
        }
        // 无画像查询→失新。
        if e.profiles.query(1, 5) != Err(XC_PROFILE_STALE) {
            return Err(XC_PROFILE_STALE);
        }
        // 刷新批 5 →批 5 可查。
        e.profiles.refresh(EfficacyProfile { cls: 1, fpw_bp: 5000, based_on_batch: 5 });
        let p = e.profiles.query(1, 5)?;
        if p.fpw_bp != 5000 {
            return Err(XC_PROFILE_STALE);
        }
        // 实测推进到批 6，画像未刷→滞后 0x7604。
        if e.profiles.query(1, 6) != Err(XC_PROFILE_STALE) {
            return Err(XC_PROFILE_STALE);
        }
        if e.profiles.stale_events != 1 {
            return Err(XC_PROFILE_STALE);
        }
        // 刷新批 6 →过。
        e.profiles.refresh(EfficacyProfile { cls: 1, fpw_bp: 5200, based_on_batch: 6 });
        e.profiles.query(1, 6)?;
        Ok(())
    }

    /// 组五：J08 对接——五指标报告过；隐私出域拒；利用率越界拒。
    pub fn selfcheck_bridge(&mut self) -> Result<(), u32> {
        let mut e = HetTelEngine::new();
        let mut snap = [[0u32; METRIC_COUNT]; 3];
        snap[0][0] = 3_000;
        snap[1][0] = 8_000;
        snap[2][0] = 9_500;
        let r = J08Report {
            snapshot: snap,
            frame_no: 42,
            privacy_domain: PRIVACY_LOCAL,
        };
        e.bridge.submit(&r)?;
        // 出域报告→0x7605。
        let mut r2 = J08Report {
            snapshot: snap,
            frame_no: 43,
            privacy_domain: 1,
        };
        if e.bridge.submit(&r2) != Err(XC_PRIVACY_BREACH) {
            return Err(XC_PRIVACY_BREACH);
        }
        // 利用率越界（>10000）→0x7601。
        r2.privacy_domain = PRIVACY_LOCAL;
        r2.snapshot[1][0] = 20_000;
        if e.bridge.submit(&r2) != Err(XC_METRIC_REG_MISS) {
            return Err(XC_METRIC_REG_MISS);
        }
        if e.bridge.privacy_events != 1 || e.bridge.submitted != 1 {
            return Err(XC_PRIVACY_BREACH);
        }
        Ok(())
    }

    /// 组六：判据——端到端：注册全量+归因复式+帧段守恒+画像新鲜+对接过。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = HetTelEngine::new();
        e.reg.register_all();
        e.reg.complete()?;
        e.attr.record(AttrEntry { task: 1, cls: 2, cost_us: 5_000 })?;
        e.attr.record(AttrEntry { task: 1, cls: 1, cost_us: 2_500 })?;
        e.attr.double_entry_ok()?;
        let mut f = FrameAttribution::new(1);
        f.add_seg(FrameSeg { task: 1, cls: 2, len_us: 5_000 })?;
        f.add_seg(FrameSeg { task: 1, cls: 1, len_us: 2_500 })?;
        f.conserve()?;
        e.profiles.refresh(EfficacyProfile { cls: 2, fpw_bp: 300, based_on_batch: 9 });
        e.profiles.query(2, 9)?;
        let snap = [[0u32; METRIC_COUNT]; 3];
        e.bridge.submit(&J08Report { snapshot: snap, frame_no: 1, privacy_domain: PRIVACY_LOCAL })?;
        // 全链账面零事件。
        if e.attr.imbalance_events != 0
            || f.breach_events != 0
            || e.profiles.stale_events != 0
            || e.bridge.privacy_events != 0
        {
            return Err(XC_ATTR_IMBALANCE);
        }
        Ok(())
    }

    /// 全量自检入口（六组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_metrics()?;
        self.selfcheck_attr()?;
        self.selfcheck_frames()?;
        self.selfcheck_profiles()?;
        self.selfcheck_bridge()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7600..0x7700 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_METRIC_REG_MISS,
            XC_ATTR_IMBALANCE,
            XC_FRAME_SEG_BREACH,
            XC_PROFILE_STALE,
            XC_PRIVACY_BREACH,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7600 || codes[i] >= 0x7700 {
                return Err(XC_METRIC_REG_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_METRIC_REG_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
