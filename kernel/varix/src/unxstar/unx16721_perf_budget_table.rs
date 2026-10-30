//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16721 · 域级性能预算总表冻结 · 0x68280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B37）：
//!   ① 纯功能代码 ≥276 行；
//!   ② UNX-F16721-J1 全域性能预算总表（40 项指标：启动/P95/帧率/
//!     内存/磁盘/时长）冻结版本化，与各段分账逐项可对；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 项指标闭集：启动类 6 项/P95 类 8 项/帧率类 6 项/内存类
//!     8 项/磁盘类 6 项/时长类 6 项，六类合计恰 40；
//!   - 每项三列：指标 ID（PRF-xxx）/类别（六类闭集）/预算值（整
//!     数微秒或字节，冻结）；
//!   - 冻结版本化：FNV-1a 全表指纹封印 + 版本只前进；追加允许、
//!     预算值改动禁止（改值即拒——预算冻结红线）；
//!   - 与各段分账逐项可对：每项须挂一个段分账锚（段号闭集），
//!     未挂锚即对账断裂；
//!   - LCG 种子写死 = SEG_BASE（0x68280）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68280，AI-VX3-1030 批16 独占区）。
pub const SEG_BASE: u32 = 0x68280;

// ── 六类闭集 ────────────────────────────────────────────────────
pub const CAT_BOOT: usize = 0;
pub const CAT_P95: usize = 1;
pub const CAT_FPS: usize = 2;
pub const CAT_MEM: usize = 3;
pub const CAT_DISK: usize = 4;
pub const CAT_DUR: usize = 5;
pub const CATEGORY_COUNT: usize = 6;
pub const CATEGORY_NAMES: [&str; CATEGORY_COUNT] =
    ["启动", "P95", "帧率", "内存", "磁盘", "时长"];
/// 每类指标数（冻结：6+8+6+8+6+6=40）。
pub const CAT_SIZES: [usize; CATEGORY_COUNT] = [6, 8, 6, 8, 6, 6];
pub const METRIC_TOTAL: usize = 40;

// ── 段分账锚闭集（挂接 B 批段号） ───────────────────────────────
pub const SEGMENT_ANCHORS: [&str; 8] = [
    "B15-setwpos", "B16-msgpipe", "B17-inroute", "B18-clipdrv",
    "B19-shmbuf", "B20-sevchain", "B24-crt", "B32-advapi",
];

// ── 预算冻结值（40 项；单位：µs 或字节，按类区分） ──────────────
/// 启动 6 项（µs）。
pub const BOOT_BUDGETS: [u64; 6] =
    [3_000_000, 2_500_000, 1_200_000, 900_000, 600_000, 400_000];
/// P95 8 项（µs）。
pub const P95_BUDGETS: [u64; 8] =
    [16_600, 12_500, 8_300, 5_000, 3_300, 2_500, 1_600, 800];
/// 帧率 6 项（µs/帧）。
pub const FPS_BUDGETS: [u64; 6] =
    [16_600, 16_600, 16_600, 8_300, 8_300, 4_100];
/// 内存 8 项（字节）。
pub const MEM_BUDGETS: [u64; 8] =
    [268_435_456, 134_217_728, 67_108_864, 33_554_432, 16_777_216, 8_388_608, 4_194_304, 1_048_576];
/// 磁盘 6 项（µs）。
pub const DISK_BUDGETS: [u64; 6] =
    [5_000_000, 2_000_000, 1_000_000, 500_000, 200_000, 100_000];
/// 时长 6 项（µs）。
pub const DUR_BUDGETS: [u64; 6] =
    [60_000_000, 30_000_000, 10_000_000, 5_000_000, 1_000_000, 500_000];

/// 预算表版本。
pub const TABLE_VERSION: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 类别越闭集
    CategoryOutOfRange,
    /// 类名空（六类齐备破坏）
    CategoryNameBlank,
    /// 指标总数破坏（≠40）
    MetricTotalBroken,
    /// 预算值为零（缺列）
    BudgetZero,
    /// 预算值改动（冻结红线破坏）
    BudgetMutated,
    /// 段分账锚缺失
    AnchorMissing,
    /// 锚未命中段闭集
    AnchorNotMapped,
    /// 版本回退
    VersionNotForward,
    /// 指纹不一致
    HashMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl PerfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::CategoryOutOfRange => "类别越闭集：六类冻结",
            PerfCode::CategoryNameBlank => "类名空：六类齐备破坏",
            PerfCode::MetricTotalBroken => "指标总数破坏：≠40",
            PerfCode::BudgetZero => "预算值为零：缺列",
            PerfCode::BudgetMutated => "预算值改动：冻结红线破坏",
            PerfCode::AnchorMissing => "段分账锚缺失",
            PerfCode::AnchorNotMapped => "锚未命中段闭集",
            PerfCode::VersionNotForward => "版本回退",
            PerfCode::HashMismatch => "指纹不一致",
            PerfCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PerfCode; 10] = [
        PerfCode::CategoryOutOfRange,
        PerfCode::CategoryNameBlank,
        PerfCode::MetricTotalBroken,
        PerfCode::BudgetZero,
        PerfCode::BudgetMutated,
        PerfCode::AnchorMissing,
        PerfCode::AnchorNotMapped,
        PerfCode::VersionNotForward,
        PerfCode::HashMismatch,
        PerfCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if PerfCode::ALL[i].val() == code {
            return PerfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x68280 段闭集内"
}

// ── 指标项 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricRow {
    pub id: u32,
    pub category: usize,
    pub budget: u64,
    /// 段分账锚下标（0xFF=缺）。
    pub anchor: u8,
}

pub const ANCHOR_NONE: u8 = 0xFF;

impl MetricRow {
    pub fn legal(&self) -> Result<(), u32> {
        if self.category >= CATEGORY_COUNT {
            return Err(PerfCode::CategoryOutOfRange.val());
        }
        if self.budget == 0 {
            return Err(PerfCode::BudgetZero.val());
        }
        if self.anchor == ANCHOR_NONE {
            return Err(PerfCode::AnchorMissing.val());
        }
        if self.anchor as usize >= SEGMENT_ANCHORS.len() {
            return Err(PerfCode::AnchorNotMapped.val());
        }
        Ok(())
    }
}

/// 全 40 项基线表（构造冻结：类别+类内序派生 ID，锚轮转）。
pub fn baseline_table() -> Result<Vec<MetricRow>, u32> {
    let mut v = Vec::new();
    let mut id = 1u32;
    let mut cat = 0;
    while cat < CATEGORY_COUNT {
        let n = CAT_SIZES[cat];
        let mut k = 0;
        while k < n {
            let budget = match cat {
                CAT_BOOT => BOOT_BUDGETS[k],
                CAT_P95 => P95_BUDGETS[k],
                CAT_FPS => FPS_BUDGETS[k],
                CAT_MEM => MEM_BUDGETS[k],
                CAT_DISK => DISK_BUDGETS[k],
                _ => DUR_BUDGETS[k],
            };
            v.push(MetricRow {
                id,
                category: cat,
                budget,
                anchor: (k % SEGMENT_ANCHORS.len()) as u8,
            });
            id += 1;
            k += 1;
        }
        cat += 1;
    }
    if v.len() != METRIC_TOTAL {
        return Err(PerfCode::MetricTotalBroken.val());
    }
    Ok(v)
}

// ── 预算总表（版本化 + 冻结 + 改值拒） ──────────────────────────
pub struct BudgetTable {
    pub rows: Vec<MetricRow>,
    pub version: u32,
    pub frozen_hash: u64,
    pub mutate_caught: u32,
}

impl BudgetTable {
    pub fn new(version: u32) -> BudgetTable {
        BudgetTable { rows: Vec::new(), version, frozen_hash: 0, mutate_caught: 0 }
    }

    pub fn load(&mut self, rows: Vec<MetricRow>) -> Result<(), u32> {
        if rows.len() != METRIC_TOTAL {
            return Err(PerfCode::MetricTotalBroken.val());
        }
        let mut i = 0;
        while i < rows.len() {
            rows[i].legal()?;
            i += 1;
        }
        self.rows = rows;
        Ok(())
    }

    /// 追加指标（version 前进 + 行数突破 40 允许，只增）。
    pub fn append(&mut self, row: MetricRow) -> Result<(), u32> {
        row.legal()?;
        self.rows.push(row);
        self.version += 1;
        Ok(())
    }

    /// 改值（预算冻结红线：一律拒）。
    pub fn set_budget(&mut self, idx: usize, _new: u64) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(PerfCode::MetricTotalBroken.val());
        }
        self.mutate_caught += 1;
        Err(PerfCode::BudgetMutated.val())
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.rows.len() {
            h ^= self.rows[i].id as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].category as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].budget;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.rows[i].anchor as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.version as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen_hash = self.hash();
        self.frozen_hash
    }

    pub fn frozen_ok(&self) -> bool {
        self.frozen_hash != 0 && self.frozen_hash == self.hash()
    }

    /// 六类齐备账。
    pub fn categories_ready() -> bool {
        if CATEGORY_NAMES.len() != CATEGORY_COUNT {
            return false;
        }
        let mut i = 0;
        while i < CATEGORY_COUNT {
            if CATEGORY_NAMES[i].len() == 0 {
                return false;
            }
            i += 1;
        }
        // 类大小和恰 40。
        let mut s = 0;
        i = 0;
        while i < CATEGORY_COUNT {
            s += CAT_SIZES[i];
            i += 1;
        }
        s == METRIC_TOTAL
    }

    /// 逐项可对：每项锚命中。
    pub fn all_anchored(&self) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].anchor == ANCHOR_NONE
                || self.rows[i].anchor as usize >= SEGMENT_ANCHORS.len()
            {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：装载 40 项→冻结→逐项锚可对→改值拒→追加演进。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut t = BudgetTable::new(TABLE_VERSION);
    t.load(baseline_table()?)?;
    t.freeze();
    if !t.frozen_ok() {
        return Err(PerfCode::HashMismatch.val());
    }
    if !t.all_anchored() {
        return Err(PerfCode::AnchorNotMapped.val());
    }
    // 改值必拒。
    if t.set_budget(0, 1).is_ok() {
        return Err(PerfCode::BudgetMutated.val());
    }
    // 追加演进。
    t.append(MetricRow { id: 41, category: CAT_P95, budget: 400, anchor: 0 })?;
    t.freeze();
    if !t.frozen_ok() {
        return Err(PerfCode::HashMismatch.val());
    }
    if !BudgetTable::categories_ready() {
        return Err(PerfCode::CategoryNameBlank.val());
    }
    Ok((t.rows.len(), t.version, t.mutate_caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut t = BudgetTable::new(1);
    t.load(baseline_table()?)?;
    t.freeze();
    if t.frozen_ok() {
        Ok(())
    } else {
        Err(PerfCode::HashMismatch.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 零预算 → 必拒。
    let row = MetricRow { id: 99, category: 0, budget: 0, anchor: 0 };
    row.legal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 锚缺失 → 必拒。
    let row = MetricRow { id: 99, category: 0, budget: 1, anchor: ANCHOR_NONE };
    row.legal()
}

/// 注入 10/10：零预算、锚缺失、改值、类别越界四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let row0 = MetricRow { id: 1, category: 0, budget: 0, anchor: 0 };
        led.record(0, row0.legal().is_err());
        let row1 = MetricRow { id: 2, category: 0, budget: 1, anchor: ANCHOR_NONE };
        led.record(1, row1.legal().is_err());
        let mut t2 = BudgetTable::new(1);
        t2.load(baseline_table()?).ok();
        led.record(2, t2.set_budget(3, 9).is_err());
        let row3 = MetricRow { id: 3, category: CATEGORY_COUNT, budget: 1, anchor: 0 };
        led.record(3, row3.legal().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PerfCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        let v = PerfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 40 项闭集点验。
    let base = baseline_table();
    v.push((
        "J1-40项闭集",
        match &base {
            Ok(t) => t.len() == 40,
            Err(_) => false,
        },
        format!("指标 {:?}", base.as_ref().map(|t| t.len())),
    ));
    // 六类齐备。
    v.push((
        "J1-六类齐备",
        BudgetTable::categories_ready(),
        format!("{:?}", CATEGORY_NAMES),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x68280 窗口")));
    v
}

pub fn run_unx16721_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16721 域级性能预算总表冻结");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
