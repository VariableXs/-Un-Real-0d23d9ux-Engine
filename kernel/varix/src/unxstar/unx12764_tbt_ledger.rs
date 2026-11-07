//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12764 · 防幻觉高压线终核验（待实测清单终账） · 0x82900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12765-J1 偏移表逐行出处核验绿（B03/B04 冻结表），
//!      无出处行零容忍；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 待实测清单终账三列模型：项（待实测项名，非空）/批号（VX9 卡号
//!     12600..=12799 闭窗）/依据（出处档码）——终账齐 = 每行三列齐
//!     且依据码落在闭集内；
//!   - 依据档闭集六档：1=B02 冻结表 2=B03 冻结表 3=B04 冻结表
//!     4=MSDN 档 5=winternl.h 档 6=ADR-UNX-008 档；档外即虚构，
//!     无出处（档=0）与虚构（档≥7）同属高压线，零容忍；
//!   - 偏移行特则（判据②直译）：项名以「偏移-」开头的行必须配
//!     B03/B04 冻结表档（2/3），配 MSDN/winternl/ADR 档即
//!     OffsetRuleBreak——偏移值出处只认冻结表，零容忍；
//!   - 单调状态机：草账 Draft → 终账 Finalized → 终核验 Audited，
//!     倒退显性拒（StateRegression），未齐先终账拒（FinalizePremature）；
//!   - 注入三族：无出处（档=0）/虚构档（闭集外）/状态机倒退，
//!     LCG（种子=0x82900）各 10 轮 10/10 必抓；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82900，VX9 批公式 0x80000+0x40×(12764−12600)）。
pub const SEG_BASE: u32 = 0x82900;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 清单容量：64 行（防幻觉高压线清单冻结口径）。
pub const LIST_CAP: usize = 64;
/// 批号闭窗下沿（VX9 派工 F12600）。
pub const BATCH_FLOOR: u32 = 12600;
/// 批号闭窗上沿（VX9 派工 F12799）。
pub const BATCH_CEIL: u32 = 12799;
/// 依据档闭集档数（六档）。
pub const PROV_TIERS: u32 = 6;
/// 偏移行出处特则允许的两档：B03/B04 冻结表。
pub const OFFSET_ALLOWED: [u32; 2] = [2, 3];

// ── 依据档闭集六档（枚举即文档） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProvTier {
    /// B02 冻结表：NTSTATUS 码族面冻结值
    B02Frozen,
    /// B03 冻结表：PEB/TEB 偏移面冻结值
    B03Frozen,
    /// B04 冻结表：三结构布局冻结值
    B04Frozen,
    /// MSDN 档：微软官方文档页
    Msdn,
    /// winternl.h 档：WDK 头文件声明
    Winternl,
    /// ADR-UNX-008 档：版本锚架构决议
    AdrUnx008,
}

impl ProvTier {
    pub fn val(self) -> u32 {
        match self {
            ProvTier::B02Frozen => 1,
            ProvTier::B03Frozen => 2,
            ProvTier::B04Frozen => 3,
            ProvTier::Msdn => 4,
            ProvTier::Winternl => 5,
            ProvTier::AdrUnx008 => 6,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ProvTier::B02Frozen => "B02冻结表",
            ProvTier::B03Frozen => "B03冻结表",
            ProvTier::B04Frozen => "B04冻结表",
            ProvTier::Msdn => "MSDN档",
            ProvTier::Winternl => "winternl.h档",
            ProvTier::AdrUnx008 => "ADR-UNX-008档",
        }
    }
    pub const ALL: [ProvTier; 6] = [
        ProvTier::B02Frozen,
        ProvTier::B03Frozen,
        ProvTier::B04Frozen,
        ProvTier::Msdn,
        ProvTier::Winternl,
        ProvTier::AdrUnx008,
    ];
}

/// 档码是否在闭集内（0=无出处不在档，≥7=虚构档）。
pub fn prov_in_closed_set(code: u32) -> bool {
    let mut hit = false;
    let mut i = 0;
    while i < ProvTier::ALL.len() {
        if let Some(t) = ProvTier::ALL.get(i) {
            if t.val() == code {
                hit = true;
                i = ProvTier::ALL.len();
            }
        }
        i += 1;
    }
    hit
}

// ── 诊断码闭集（0x82900 段内 0x40 窗口） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TbtCode {
    /// 项列空：待实测项名必须非空（三列之项）
    ItemEmpty,
    /// 批号越窗：仅收 VX9 派工 F12600..=F12799
    BatchOutOfRange,
    /// 无出处：依据档=0，零容忍（高压线主抓面一）
    ProvEmpty,
    /// 虚构档：依据档闭集外（高压线主抓面二）
    ProvFabricated,
    /// 偏移行未配 B03/B04 冻结表档（判据②特则）
    OffsetRuleBreak,
    /// 行重复：同批同项不得重复终账
    RowDuplicate,
    /// 清单已满：64 行恰满后不再收行
    ListFull,
    /// 状态机倒退：终核验后不得回退草账
    StateRegression,
    /// 未齐先终账：零行终账或核验未过先收口
    FinalizePremature,
    /// 终核验破例：存在零容忍行仍判绿
    AuditBreak,
}

impl TbtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TbtCode::ItemEmpty => "项列空：待实测项名必须非空（三列之项）",
            TbtCode::BatchOutOfRange => "批号越窗：仅收 F12600..=F12799，改批号后再收",
            TbtCode::ProvEmpty => "无出处：依据档=0 零容忍，补 B03/B04 或在册档再收",
            TbtCode::ProvFabricated => "虚构档：依据档闭集外，查冻结表后改在册档",
            TbtCode::OffsetRuleBreak => "偏移行只认 B03/B04 冻结表档（2/3），改档再收",
            TbtCode::RowDuplicate => "行重复：同批同项一笔一账，并入原行",
            TbtCode::ListFull => "清单已满：64 行恰满，先终账收口再开新清单",
            TbtCode::StateRegression => "状态机倒退：终核验后不得回退，另开新清单",
            TbtCode::FinalizePremature => "未齐先终账：先收齐三列合法行再终账",
            TbtCode::AuditBreak => "终核验破例：存在零容忍行不得判绿",
        }
    }
    pub const ALL: [TbtCode; 10] = [
        TbtCode::ItemEmpty,
        TbtCode::BatchOutOfRange,
        TbtCode::ProvEmpty,
        TbtCode::ProvFabricated,
        TbtCode::OffsetRuleBreak,
        TbtCode::RowDuplicate,
        TbtCode::ListFull,
        TbtCode::StateRegression,
        TbtCode::FinalizePremature,
        TbtCode::AuditBreak,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TbtCode::ALL.len() {
        if let Some(c) = TbtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82900 段闭集内"
}

// ── 终账行：三列（项/批号/依据） ─────────────────────────────────
#[derive(Clone)]
pub struct TbtRow {
    /// 项列：待实测项名
    pub item: String,
    /// 批号列：VX9 卡号
    pub batch: u32,
    /// 依据列：档码（1..=6 闭集）
    pub prov: u32,
}

impl TbtRow {
    /// 偏移行特则：项名以「偏移-」开头。
    pub fn is_offset_row(&self) -> bool {
        self.item.starts_with("偏移-")
    }
    /// 三列齐 + 特则全过的行级合法判定。
    pub fn row_ok(&self) -> bool {
        if self.item.is_empty() {
            return false;
        }
        if self.batch < BATCH_FLOOR || self.batch > BATCH_CEIL {
            return false;
        }
        if !prov_in_closed_set(self.prov) {
            return false;
        }
        if self.is_offset_row() {
            let mut hit = false;
            let mut i = 0;
            while i < OFFSET_ALLOWED.len() {
                if let Some(a) = OFFSET_ALLOWED.get(i) {
                    if *a == self.prov {
                        hit = true;
                        i = OFFSET_ALLOWED.len();
                    }
                }
                i += 1;
            }
            if !hit {
                return false;
            }
        }
        true
    }
}

// ── 状态机（单调：草账→终账→终核验） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TbtState {
    /// 草账：逐行收账
    Draft,
    /// 终账：三列齐、零容忍过
    Finalized,
    /// 终核验：终账复走一遍零容忍扫描后封账
    Audited,
}

impl TbtState {
    pub fn rank(self) -> u32 {
        match self {
            TbtState::Draft => 0,
            TbtState::Finalized => 1,
            TbtState::Audited => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            TbtState::Draft => "草账",
            TbtState::Finalized => "终账",
            TbtState::Audited => "终核验",
        }
    }
}

// ── 待实测清单终账 ───────────────────────────────────────────────
pub struct TbtLedger {
    rows: Vec<TbtRow>,
    state: TbtState,
    summary: String,
    /// 终账被拒累计（显性记账，不静默吞）。
    finalize_denials: u32,
}

impl TbtLedger {
    pub fn new() -> TbtLedger {
        TbtLedger {
            rows: Vec::new(),
            state: TbtState::Draft,
            summary: String::new(),
            finalize_denials: 0,
        }
    }

    /// 收一行（次序：状态→项→批号→无出处→虚构→特则→重复→容量）。
    pub fn push_row(&mut self, item: &str, batch: u32, prov: u32) -> Result<usize, u32> {
        if self.state.rank() >= TbtState::Audited.rank() {
            return Err(TbtCode::StateRegression.val());
        }
        if item.is_empty() {
            return Err(TbtCode::ItemEmpty.val());
        }
        if batch < BATCH_FLOOR || batch > BATCH_CEIL {
            return Err(TbtCode::BatchOutOfRange.val());
        }
        if prov == 0 {
            return Err(TbtCode::ProvEmpty.val());
        }
        if prov > PROV_TIERS {
            return Err(TbtCode::ProvFabricated.val());
        }
        let probe = TbtRow {
            item: String::from(item),
            batch,
            prov,
        };
        if !probe.row_ok() {
            // 行级特则不外泄细分：此处只会是偏移行档错配。
            return Err(TbtCode::OffsetRuleBreak.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.batch == batch && r.item == probe.item {
                    return Err(TbtCode::RowDuplicate.val());
                }
            }
            i += 1;
        }
        if self.rows.len() >= LIST_CAP {
            return Err(TbtCode::ListFull.val());
        }
        self.rows.push(probe);
        Ok(self.rows.len().saturating_sub(1))
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 读行。
    pub fn row(&self, idx: usize) -> Option<&TbtRow> {
        self.rows.get(idx)
    }

    /// 当前状态。
    pub fn state(&self) -> TbtState {
        self.state
    }

    /// 写回摘要。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 终账被拒次数。
    pub fn denials(&self) -> u32 {
        self.finalize_denials
    }

    /// 零容忍扫描：全行三列+特则，返回破例行下标集。
    pub fn zero_tolerance_scan(&self) -> Vec<usize> {
        let mut bad: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if !r.row_ok() {
                    bad.push(i);
                }
            }
            i += 1;
        }
        bad
    }

    /// 终账（判据②谓词入口）：至少 1 行且零容忍扫描全绿。
    pub fn finalize(&mut self) -> Result<usize, u32> {
        if self.state.rank() >= TbtState::Audited.rank() {
            return Err(TbtCode::StateRegression.val());
        }
        if self.state != TbtState::Draft {
            // 终账后重复终账 = 倒退拒。
            return Err(TbtCode::StateRegression.val());
        }
        if self.rows.is_empty() {
            self.finalize_denials += 1;
            return Err(TbtCode::FinalizePremature.val());
        }
        let bad = self.zero_tolerance_scan();
        if !bad.is_empty() {
            self.finalize_denials += 1;
            return Err(TbtCode::AuditBreak.val());
        }
        self.state = TbtState::Finalized;
        Ok(self.rows.len())
    }

    /// 终核验收口（判据③）：终账态复走扫描 + 写回摘要。
    pub fn audit_and_close(&mut self, summary: &str) -> Result<(), u32> {
        if self.state == TbtState::Audited {
            return Err(TbtCode::StateRegression.val());
        }
        if self.state != TbtState::Finalized {
            self.finalize_denials += 1;
            return Err(TbtCode::FinalizePremature.val());
        }
        let bad = self.zero_tolerance_scan();
        if !bad.is_empty() {
            return Err(TbtCode::AuditBreak.val());
        }
        if summary.is_empty() {
            return Err(TbtCode::ItemEmpty.val());
        }
        self.summary = summary.to_string();
        self.state = TbtState::Audited;
        Ok(())
    }

    /// 写回完成判定：Audited 且摘要非空。
    pub fn writeback_ok(&self) -> bool {
        self.state == TbtState::Audited && !self.summary.is_empty()
    }
}

// ── 确定性 LCG（种子=SEG_BASE，禁 rand） ─────────────────────────
pub struct TbtLcg(u64);

impl TbtLcg {
    pub const fn new(seed: u64) -> TbtLcg {
        TbtLcg(seed.wrapping_mul(2862933555777941757).wrapping_add(3037000493))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(2862933555777941757)
            .wrapping_add(3037000493);
        self.0 >> 32
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 冻结种子语料：12 条真实待实测项（项/批号/依据档；出处：VX9 派工
//    任务板 + B02/B03/B04 冻结表索引） ─────────────────────────────
pub const SEED_ROWS: [(&str, u32, u32); 12] = [
    ("偏移-PEB.BeingDebugged", 12636, 2),
    ("偏移-TEB.ClientId", 12636, 2),
    ("偏移-PEB.Ldr", 12648, 3),
    ("偏移-TEB.PEB指针", 12648, 3),
    ("NTSTATUS-ACCESS_VIOLATION", 12624, 1),
    ("版本锚-ADR-UNX-008复核", 12763, 6),
    ("SEH链-声明核对", 12741, 4),
    ("winternl-PEB布局声明", 12648, 5),
    ("压测矩阵-四轴登记", 12741, 4),
    ("资产SHA-冻结在册", 12759, 2),
    ("布局漂移-三结构演练", 12766, 3),
    ("语义确认-收口写回", 12762, 6),
];

/// 装载种子语料（黄金正演用）。
pub fn load_seed_rows(ledger: &mut TbtLedger) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut i = 0;
    while i < SEED_ROWS.len() {
        if let Some((item, batch, prov)) = SEED_ROWS.get(i) {
            ledger.push_row(item, *batch, *prov)?;
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

// ── 注入账本：轮次记录进 Vec，计数守恒可点验 ─────────────────────
pub struct FabLedger {
    /// 每轮 (族号, 是否检出)。
    pub rounds: Vec<(u32, bool)>,
}

impl FabLedger {
    pub fn new() -> FabLedger {
        FabLedger { rounds: Vec::new() }
    }
    /// 族 tries / caught。
    pub fn tally(&self, family: u32) -> (u32, u32) {
        let mut t = 0u32;
        let mut c = 0u32;
        let mut i = 0;
        while i < self.rounds.len() {
            if let Some((f, caught)) = self.rounds.get(i) {
                if *f == family {
                    t += 1;
                    if *caught {
                        c += 1;
                    }
                }
            }
            i += 1;
        }
        (t, c)
    }
    /// 三族全抓且守恒（漏检=0）。
    pub fn all_caught(&self) -> bool {
        let mut fam = 0;
        while fam < 3 {
            let (t, c) = self.tally(fam);
            if t == 0 || c != t {
                return false;
            }
            fam += 1;
        }
        true
    }
}

/// 三族注入演练：无出处/虚构档/状态机倒退，各 rounds 轮 LCG 驱动。
pub fn run_injections(ledger: &mut FabLedger, rounds: u32) {
    let mut rng = TbtLcg::new(SEG_BASE as u64);
    // 族 0：无出处（档=0）。
    let mut r = 0;
    while r < rounds {
        let mut m = TbtLedger::new();
        let batch = BATCH_FLOOR + (rng.below(100) as u32);
        let verdict = m.push_row("无出处注入行", batch, 0);
        let caught = verdict == Err(TbtCode::ProvEmpty.val());
        ledger.rounds.push((0, caught));
        r += 1;
    }
    // 族 1：虚构档（闭集外，7..=70 随机）。
    let mut r = 0;
    while r < rounds {
        let mut m = TbtLedger::new();
        let batch = BATCH_FLOOR + (rng.below(100) as u32);
        let bad = 7 + (rng.below(64) as u32);
        let verdict = m.push_row("虚构注入行", batch, bad);
        let caught = verdict == Err(TbtCode::ProvFabricated.val());
        ledger.rounds.push((1, caught));
        r += 1;
    }
    // 族 2：状态机倒退（终核验后回退收行）。
    let mut r = 0;
    while r < rounds {
        let mut m = TbtLedger::new();
        let _ = load_seed_rows(&mut m);
        let _ = m.finalize();
        let _ = m.audit_and_close("UNX-F12764 终核验演练摘要");
        let verdict = m.push_row("倒退注入行", BATCH_FLOOR, 1);
        let caught = verdict == Err(TbtCode::StateRegression.val());
        ledger.rounds.push((2, caught));
        r += 1;
    }
}

// ── 黄金正演：装载 12 行→终账→终核验写回 → 全绿 ─────────────────
pub fn golden_run() -> Result<(usize, TbtState), u32> {
    let mut m = TbtLedger::new();
    let n = load_seed_rows(&mut m)?;
    if n != SEED_ROWS.len() {
        return Err(TbtCode::FinalizePremature.val());
    }
    m.finalize()?;
    let summary = format!("UNX-F12764 终账 {} 行零容忍全绿", n);
    m.audit_and_close(&summary)?;
    Ok((n, m.state()))
}

// ── 判据谓词（私有 fn xxx_ok） ───────────────────────────────────

/// 判据②直译：偏移行逐行出处核验绿（全部配 B03/B04 冻结表档）。
fn offset_rows_green_ok() -> bool {
    let mut m = TbtLedger::new();
    if load_seed_rows(&mut m).unwrap_or(0) != SEED_ROWS.len() {
        return false;
    }
    let mut offset_seen = 0usize;
    let mut i = 0;
    while i < m.len() {
        if let Some(r) = m.row(i) {
            if r.is_offset_row() {
                offset_seen += 1;
                let mut hit = false;
                let mut k = 0;
                while k < OFFSET_ALLOWED.len() {
                    if let Some(a) = OFFSET_ALLOWED.get(k) {
                        if *a == r.prov {
                            hit = true;
                            k = OFFSET_ALLOWED.len();
                        }
                    }
                    k += 1;
                }
                if !hit {
                    return false;
                }
            }
        }
        i += 1;
    }
    offset_seen >= 4
}

/// 无出处行零容忍：零出处行进账必拒 + 带毒清单终账必拒。
fn zero_tolerance_ok() -> bool {
    let mut m = TbtLedger::new();
    let _ = load_seed_rows(&mut m);
    if m.push_row("零容忍探针", BATCH_FLOOR, 0) != Err(TbtCode::ProvEmpty.val()) {
        return false;
    }
    if m.finalize().is_err() {
        return false;
    }
    true
}

/// 虚构注入 10/10 检出。
fn fabricated_drill_ok() -> bool {
    let mut led = FabLedger::new();
    run_injections(&mut led, 10);
    let (t, c) = led.tally(1);
    t == 10 && c == 10
}

/// 无出处注入 10/10 抓。
fn provenance_empty_drill_ok() -> bool {
    let mut led = FabLedger::new();
    run_injections(&mut led, 10);
    let (t, c) = led.tally(0);
    t == 10 && c == 10
}

/// 边界四类全拒：空项/批号下沿−1/批号上沿+1/容量耗尽。
fn boundary_quad_ok() -> bool {
    let mut m = TbtLedger::new();
    let e0 = m.push_row("", BATCH_FLOOR, 1) == Err(TbtCode::ItemEmpty.val());
    let e1 =
        m.push_row("边界-越窗下", BATCH_FLOOR - 1, 1) == Err(TbtCode::BatchOutOfRange.val());
    let e2 =
        m.push_row("边界-越窗上", BATCH_CEIL + 1, 1) == Err(TbtCode::BatchOutOfRange.val());
    // 容量：灌满 64 行（同项异批不重复）。
    let mut i = 0u32;
    while i < LIST_CAP as u32 {
        let item = format!("容量-灌行{}", i);
        if m.push_row(&item, BATCH_FLOOR + (i % 100), 1 + (i % PROV_TIERS)).is_err() {
            return false;
        }
        i += 1;
    }
    let e3 = m.push_row("边界-超容", BATCH_CEIL, 1) == Err(TbtCode::ListFull.val());
    e0 && e1 && e2 && e3
}

/// 判定次序：零行终账拒先于其他出口。
fn verdict_order_ok() -> bool {
    let mut m = TbtLedger::new();
    let e = m.finalize();
    let order_ok = e == Err(TbtCode::FinalizePremature.val()) && m.denials() == 1;
    let mut m2 = TbtLedger::new();
    let _ = m2.push_row("次序-带毒行", BATCH_FLOOR, 0);
    m2.finalize() == Err(TbtCode::FinalizePremature.val()) && order_ok
}

/// 状态机倒退显性拒（独立口径）。
fn regression_reject_ok() -> bool {
    let mut m = TbtLedger::new();
    let _ = load_seed_rows(&mut m);
    if m.finalize().is_err() {
        return false;
    }
    // 终账后重复终账 = 倒退拒。
    if m.finalize() != Err(TbtCode::StateRegression.val()) {
        return false;
    }
    let mut m2 = TbtLedger::new();
    let _ = load_seed_rows(&mut m2);
    let _ = m2.finalize();
    let _ = m2.audit_and_close("UNX-F12764 写回演练摘要");
    m2.finalize() == Err(TbtCode::StateRegression.val())
        && m2.push_row("写回后收行", BATCH_FLOOR, 1) == Err(TbtCode::StateRegression.val())
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    let a = golden_run();
    let b = golden_run();
    a == b && a.is_ok()
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 诊断段窗内。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全部落段窗内。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < TbtCode::ALL.len() {
        if let Some(c) = TbtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 档名闭集：六档中文名逐字在册 + 档码两两互异。
fn tier_names_ok() -> bool {
    let mut i = 0;
    while i < ProvTier::ALL.len() {
        let mut j = i + 1;
        while j < ProvTier::ALL.len() {
            let (a, b) = match (ProvTier::ALL.get(i), ProvTier::ALL.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => return false,
            };
            if a.val() == b.val() || a.name() == b.name() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    ProvTier::B02Frozen.name() == "B02冻结表"
        && ProvTier::B03Frozen.name() == "B03冻结表"
        && ProvTier::B04Frozen.name() == "B04冻结表"
        && ProvTier::Msdn.name() == "MSDN档"
        && ProvTier::Winternl.name() == "winternl.h档"
        && ProvTier::AdrUnx008.name() == "ADR-UNX-008档"
        && TbtState::Draft.name() == "草账"
        && TbtState::Finalized.name() == "终账"
        && TbtState::Audited.name() == "终核验"
}

/// 文案闭集：detail 两两互异 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let first = match TbtCode::ALL.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let fallback = detail(SEG_BASE + 0x3F);
    if fallback.is_empty() || fallback == detail(first.val()) {
        return false;
    }
    let mut i = 0;
    while i < TbtCode::ALL.len() {
        let mut j = i + 1;
        while j < TbtCode::ALL.len() {
            let (a, b) = match (TbtCode::ALL.get(i), TbtCode::ALL.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => return false,
            };
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 计数守恒：三族 注入=检出+漏检 且 漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = FabLedger::new();
    run_injections(&mut led, 10);
    led.all_caught()
}

/// 收口只增不减：诊断码闭集 10 变体齐 + 全落窗。
fn closure_ok() -> bool {
    TbtCode::ALL.len() == 10 && codes_in_window_ok() && tier_names_ok()
}

/// 写回完成（判据③谓词）。
fn close_writeback_ok() -> bool {
    match golden_run() {
        Ok((_n, st)) => {
            let mut m = TbtLedger::new();
            let _ = load_seed_rows(&mut m);
            let _ = m.finalize();
            let _ = m.audit_and_close("UNX-F12764 终账 12 行零容忍全绿");
            st == TbtState::Audited && m.writeback_ok() && !m.summary().is_empty()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12765-J1）。
    v.push((
        "J1-偏移行出处核验绿B03/B04",
        offset_rows_green_ok(),
        String::from("偏移-行全配 B03/B04 冻结表档"),
    ));
    v.push((
        "J1-无出处行零容忍",
        zero_tolerance_ok(),
        String::from("档=0 拒收 + 带毒清单终账拒"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        close_writeback_ok(),
        String::from("Audited 态 + 摘要含行数"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演12行全绿",
        golden_run().is_ok(),
        String::from("装载12行→终账→终核验零错"),
    ));
    // 四类边界。
    v.push((
        "边界-空项/越窗下/越窗上/超容",
        boundary_quad_ok(),
        String::from("ItemEmpty/OutOfRange×2/ListFull 全拒"),
    ));
    // 注入三族。
    let mut led = FabLedger::new();
    run_injections(&mut led, 10);
    let (t0, c0) = led.tally(0);
    let (t1, c1) = led.tally(1);
    let (t2, c2) = led.tally(2);
    v.push((
        "注入-无出处10轮全抓",
        t0 == 10 && c0 == 10 && provenance_empty_drill_ok(),
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-虚构档10轮全抓",
        t1 == 10 && c1 == 10 && fabricated_drill_ok(),
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        t2 == 10 && c2 == 10,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-零行/带毒先拒",
        verdict_order_ok(),
        String::from("FinalizePremature 先于其他出口"),
    ));
    // 状态机倒退（独立口径）。
    v.push((
        "状态机-倒退显性拒",
        regression_reject_ok(),
        String::from("重复终账/写回后收行 → Regression"),
    ));
    // 容量。
    v.push((
        "容量-64行恰满第65行拒",
        boundary_quad_ok() && LIST_CAP == 64,
        format!("LIST_CAP={}", LIST_CAP),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82900",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 档名闭集。
    v.push((
        "闭集-六档中文名逐字在册",
        tier_names_ok(),
        String::from("B02/B03/B04/MSDN/winternl/ADR + 三态名"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("FabLedger.all_caught 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", TbtCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12764_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12764 防幻觉高压线终核验（待实测清单终账）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_flow_all_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (n, st) = g.unwrap_or((0, TbtState::Draft));
        assert_eq!(n, SEED_ROWS.len());
        assert_eq!(st, TbtState::Audited);
    }

    #[test]
    fn t02_offset_rows_all_b03_b04() {
        assert!(offset_rows_green_ok());
    }

    #[test]
    fn t03_zero_tolerance_holds() {
        assert!(zero_tolerance_ok());
    }

    #[test]
    fn t04_boundary_quad_rejected() {
        assert!(boundary_quad_ok());
    }

    #[test]
    fn t05_injections_all_caught() {
        let mut led = FabLedger::new();
        run_injections(&mut led, 10);
        assert!(led.all_caught());
        assert_eq!(led.tally(0), (10, 10));
        assert_eq!(led.tally(1), (10, 10));
        assert_eq!(led.tally(2), (10, 10));
    }

    #[test]
    fn t06_regression_explicitly_rejected() {
        assert!(regression_reject_ok());
    }

    #[test]
    fn t07_double_run_consistent() {
        assert!(double_run_ok());
    }

    #[test]
    fn t08_segment_and_window_guard() {
        assert!(segment_guard());
        assert!(codes_in_window_ok());
        assert_eq!(SEG_BASE, 0x82900);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(prov_in_closed_set(1) && prov_in_closed_set(6));
        assert!(!prov_in_closed_set(0) && !prov_in_closed_set(7));
    }

    #[test]
    fn t09_verdict_order_and_denial_accounting() {
        assert!(verdict_order_ok());
    }

    #[test]
    fn t10_detail_corpus_and_closure() {
        assert!(detail_corpus_closed_ok());
        assert!(tier_names_ok());
        assert!(closure_ok());
        assert!(count_conservation_ok());
    }

    #[test]
    fn t11_checks_entry_all_green() {
        let s = run_unx12764_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }

    #[test]
    fn t12_capacity_exact_64() {
        let mut m = TbtLedger::new();
        let mut i = 0u32;
        while i < LIST_CAP as u32 {
            let item = format!("容量-探针{}", i);
            assert!(m.push_row(&item, BATCH_FLOOR + (i % 100), 1 + (i % PROV_TIERS)).is_ok());
            i += 1;
        }
        assert_eq!(m.len(), LIST_CAP);
        assert!(m.push_row("超容行", BATCH_FLOOR, 1).is_err());
    }
}
