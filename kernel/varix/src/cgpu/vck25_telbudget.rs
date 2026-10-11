//! CGPU-F1706 · 异构遥测性能预算
//!
//! 判据锚点（施工书原文摘录）：
//! 性能预算：开销（采集/分析/呈现三段开销预算（复用 F1565 模式——预算表（异构
//! 增量≤0.1ms——增量门禁；零干扰（复用三条——干扰防护复用；降级（资源紧张→
//! 采样降级——降级复用；自观测（自举观测复用；测试（预算/干扰/降级/自观测四组）。
//! 判据：增量 0.1ms、三条、降级、自举、四组、判据。
//!
//! 诊断码段 0x8F01~0x8F05（独占）：
//!   0x8F01 增量门禁破（三段任一段超预算/总增量>100µs）
//!   0x8F02 零干扰防护破（三防护条任一失守）
//!   0x8F03 降级失序（采样档越闭集/降级不按序回升即跳）
//!   0x8F04 自举失真（自观测自身开销未入账/观测器观测自身死循环）
//!   0x8F05 段越界（判码落 0x8F00..0x9000 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const TEL_BUDGET_VERSION: &str = "K25-telbudget-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8F01~0x8F05）
// ---------------------------------------------------------------------------

pub const XC_INCREMENT_BREACH: u32 = 0x8F01;
pub const XC_INTERFERENCE_BREACH: u32 = 0x8F02;
pub const XC_DEGRADE_MISORDER: u32 = 0x8F03;
pub const XC_BOOTSTRAP_DISTORTED: u32 = 0x8F04;
pub const XC_SEG_OOR: u32 = 0x8F05;

// ---------------------------------------------------------------------------
// 一、三段开销预算表（F1565 模式复用）
// ---------------------------------------------------------------------------

/// 预算段三闭集（判据"采集/分析/呈现三段"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetStage {
    /// 采集：探针读数+打包。
    Collect,
    /// 分析：归因/画像折算。
    Analyze,
    /// 呈现：面板行组装。
    Present,
}

impl BudgetStage {
    pub const ALL: [BudgetStage; 3] =
        [BudgetStage::Collect, BudgetStage::Analyze, BudgetStage::Present];
    /// 各段预算 µs（异构增量总门禁 100µs 分账：50/30/20）。
    pub const fn budget_us(self) -> u64 {
        match self {
            BudgetStage::Collect => 50,
            BudgetStage::Analyze => 30,
            BudgetStage::Present => 20,
        }
    }
}

/// 总增量门禁（判据"增量≤0.1ms"= 100µs）。
pub const TOTAL_INCREMENT_BUDGET_US: u64 = 100;

/// 预算账：三段实测入账 + 门禁判定。
pub struct BudgetBook {
    /// 三段实测 µs。
    pub measured: [u64; 3],
}

impl BudgetBook {
    pub fn new() -> Self {
        BudgetBook { measured: [0; 3] }
    }

    /// 入账：段序合法。
    pub fn record(&mut self, s: BudgetStage, us: u64) -> Result<(), u32> {
        if us > 12_500 {
            return Err(XC_INCREMENT_BREACH);
        }
        self.measured[s as usize] += us;
        Ok(())
    }

    /// 门禁：单段不超段预算 + 三段总和不超 100µs（0x8F01）。
    pub fn gated(&self) -> Result<(), u32> {
        let mut total = 0;
        let mut i = 0;
        while i < 3 {
            if self.measured[i] > BudgetStage::ALL[i].budget_us() {
                return Err(XC_INCREMENT_BREACH);
            }
            total += self.measured[i];
            i += 1;
        }
        if total > TOTAL_INCREMENT_BUDGET_US {
            return Err(XC_INCREMENT_BREACH);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、零干扰防护（复用三条）
// ---------------------------------------------------------------------------

/// 三防护条闭集（判据"零干扰（复用三条"）。
pub const GUARDS: [&str; 3] = [
    "G1-no-alloc-in-hot-path",  // 热路径零分配（采样器预分配缓冲）
    "G2-no-lock-in-hot-path",   // 热路径零锁（环形缓冲单写单读）
    "G3-budget-capped",         // 采样器自身有预算帽（不因观测拖垮被观测）
];

/// 干扰防护审计：三防护条全在册即守恒。
pub fn interference_audit(active: &[&str]) -> Result<(), u32> {
    if active.len() < GUARDS.len() {
        return Err(XC_INTERFERENCE_BREACH);
    }
    for g in GUARDS {
        let mut found = false;
        for a in active {
            if *a == g {
                found = true;
            }
        }
        if !found {
            return Err(XC_INTERFERENCE_BREACH);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、采样降级（资源紧张→采样降级——降级复用）
// ---------------------------------------------------------------------------

/// 采样档四闭集（全采→1/2→1/4→停采；降级按序不跳档）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SampleTier {
    Full,
    Half,
    Quarter,
    Off,
}

impl SampleTier {
    pub const ALL: [SampleTier; 4] =
        [SampleTier::Full, SampleTier::Half, SampleTier::Quarter, SampleTier::Off];
    /// 采万分比（Full=10000 … Off=0）。
    pub const fn rate_bp(self) -> u32 {
        match self {
            SampleTier::Full => 10_000,
            SampleTier::Half => 5_000,
            SampleTier::Quarter => 2_500,
            SampleTier::Off => 0,
        }
    }
}

/// 降级器：按序降级/按序回升。
pub struct Degrader {
    pub tier: SampleTier,
    /// 降级事件数/回升事件数（审计面）。
    pub down_events: u32,
    pub up_events: u32,
}

impl Degrader {
    pub fn new() -> Self {
        Degrader { tier: SampleTier::Full, down_events: 0, up_events: 0 }
    }

    /// 降一档（不得跳档——相邻才行）。
    pub fn step_down(&mut self) -> Result<(), u32> {
        let cur = self.tier as usize;
        if cur + 1 >= SampleTier::ALL.len() {
            return Err(XC_DEGRADE_MISORDER); // 已在 Off，不能再降。
        }
        self.tier = SampleTier::ALL[cur + 1];
        self.down_events += 1;
        Ok(())
    }

    /// 回升一档（不得跳档）。
    pub fn step_up(&mut self) -> Result<(), u32> {
        let cur = self.tier as usize;
        if cur == 0 {
            return Err(XC_DEGRADE_MISORDER);
        }
        self.tier = SampleTier::ALL[cur - 1];
        self.up_events += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、自举观测（自观测复用）
// ---------------------------------------------------------------------------

/// 自举账：观测器自身开销必须入自身账（观测自己不豁免——自举诚实）。
pub struct Bootstrap {
    /// 观测器自身每帧开销 µs。
    pub self_overhead_us: u64,
    /// 是否已入预算账。
    pub booked: bool,
    /// 观测深度哨（防观测器观测自身死循环——深度帽 1）。
    pub depth: u8,
}

/// 自举深度帽。
pub const BOOTSTRAP_MAX_DEPTH: u8 = 1;

impl Bootstrap {
    pub fn new() -> Self {
        Bootstrap { self_overhead_us: 6, booked: false, depth: 0 }
    }

    /// 自举观测：深度+1，越帽拒（0x8F04）。
    pub fn enter(&mut self) -> Result<(), u32> {
        if self.depth >= BOOTSTRAP_MAX_DEPTH {
            return Err(XC_BOOTSTRAP_DISTORTED);
        }
        self.depth += 1;
        Ok(())
    }

    pub fn exit(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 自举入账：自身开销并入预算账（未入账即失真）。
    pub fn book_self(&mut self, book: &mut BudgetBook) -> Result<(), u32> {
        if self.booked {
            return Err(XC_BOOTSTRAP_DISTORTED); // 重复入账=重复计费。
        }
        if self.self_overhead_us == 0 {
            return Err(XC_BOOTSTRAP_DISTORTED); // 零开销自观测=撒谎。
        }
        book.record(BudgetStage::Collect, self.self_overhead_us)?;
        self.booked = true;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：预算/干扰/降级/自观测）
// ---------------------------------------------------------------------------

/// 预算引擎。
pub struct BudgetEngine {
    pub book: BudgetBook,
    pub degrader: Degrader,
    pub boot: Bootstrap,
}

impl BudgetEngine {
    pub fn new() -> Self {
        BudgetEngine { book: BudgetBook::new(), degrader: Degrader::new(), boot: Bootstrap::new() }
    }

    /// 组一：预算——三段恰值过 + 单段超拒 + 总和超拒。
    pub fn selfcheck_budget(&mut self) -> Result<(), u32> {
        let mut b = BudgetBook::new();
        b.record(BudgetStage::Collect, 50)?;
        b.record(BudgetStage::Analyze, 30)?;
        b.record(BudgetStage::Present, 20)?;
        b.gated()?; // 恰值全过（总 100 = 门禁）。
        // 单段超拒。
        let mut b2 = BudgetBook::new();
        b2.record(BudgetStage::Collect, 51)?;
        if b2.gated().is_ok() {
            return Err(XC_INCREMENT_BREACH);
        }
        // 总和超拒（单段都在限内、合计超）——三段预算 50/30/20 合计恰 100，
        // 总和超只能由单段越限引发？不：门禁分账各自相加=100 恰等于总门禁，
        // 单段全合规则总和必 ≤100——所以总和独立超测需构造越限段。用越限段
        // 复核总和路径被拦即可。
        let mut b3 = BudgetBook::new();
        b3.record(BudgetStage::Analyze, 31)?;
        b3.record(BudgetStage::Present, 70)?;
        if b3.gated().is_ok() {
            return Err(XC_INCREMENT_BREACH);
        }
        Ok(())
    }

    /// 组二：干扰——三防护全在过 + 缺一拒。
    pub fn selfcheck_interference(&mut self) -> Result<(), u32> {
        interference_audit(&GUARDS)?;
        // 缺 G2 拒。
        let partial = ["G1-no-alloc-in-hot-path", "G3-budget-capped"];
        if interference_audit(&partial).is_ok() {
            return Err(XC_INTERFERENCE_BREACH);
        }
        Ok(())
    }

    /// 组三：降级——逐档降/逐档升 + 跳档拒 + Off 见底拒 + 速率对拍。
    pub fn selfcheck_degrade(&mut self) -> Result<(), u32> {
        let mut d = Degrader::new();
        // 全采→半→季→停（三步）。
        d.step_down()?;
        d.step_down()?;
        d.step_down()?;
        if d.tier != SampleTier::Off || d.down_events != 3 {
            return Err(XC_DEGRADE_MISORDER);
        }
        // Off 见底再降拒。
        if d.step_down().is_ok() {
            return Err(XC_DEGRADE_MISORDER);
        }
        // 逐档回升。
        d.step_up()?;
        d.step_up()?;
        d.step_up()?;
        if d.tier != SampleTier::Full || d.up_events != 3 {
            return Err(XC_DEGRADE_MISORDER);
        }
        // Full 再升拒。
        if d.step_up().is_ok() {
            return Err(XC_DEGRADE_MISORDER);
        }
        // 速率闭集对拍。
        if SampleTier::Full.rate_bp() != 10_000
            || SampleTier::Half.rate_bp() != 5_000
            || SampleTier::Quarter.rate_bp() != 2_500
            || SampleTier::Off.rate_bp() != 0
        {
            return Err(XC_DEGRADE_MISORDER);
        }
        Ok(())
    }

    /// 组四：自观测——入账诚实 + 重复入账拒 + 零开销拒 + 深度帽。
    pub fn selfcheck_bootstrap(&mut self) -> Result<(), u32> {
        let mut b = BudgetBook::new();
        let mut boot = Bootstrap::new();
        boot.book_self(&mut b)?;
        if !boot.booked {
            return Err(XC_BOOTSTRAP_DISTORTED);
        }
        // 重复入账拒。
        if boot.book_self(&mut b).is_ok() {
            return Err(XC_BOOTSTRAP_DISTORTED);
        }
        // 零开销自观测拒。
        let mut lie = Bootstrap::new();
        lie.self_overhead_us = 0;
        if lie.book_self(&mut b).is_ok() {
            return Err(XC_BOOTSTRAP_DISTORTED);
        }
        // 深度帽：一次进入后再次进入拒，退出后可再进。
        let mut deep = Bootstrap::new();
        deep.enter()?;
        if deep.enter().is_ok() {
            return Err(XC_BOOTSTRAP_DISTORTED);
        }
        deep.exit();
        deep.enter()?;
        // 自身开销入账后 Collect 段水位：6µs（<50 预算）。
        if b.measured[0] < 6 {
            return Err(XC_BOOTSTRAP_DISTORTED);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_budget()?;
        self.selfcheck_interference()?;
        self.selfcheck_degrade()?;
        self.selfcheck_bootstrap()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8F00..0x9000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_INCREMENT_BREACH,
            XC_INTERFERENCE_BREACH,
            XC_DEGRADE_MISORDER,
            XC_BOOTSTRAP_DISTORTED,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8F00 || codes[i] >= 0x9000 {
                return Err(XC_INCREMENT_BREACH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_INCREMENT_BREACH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
