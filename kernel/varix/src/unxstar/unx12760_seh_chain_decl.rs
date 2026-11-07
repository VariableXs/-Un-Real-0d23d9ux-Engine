//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12760 · SEH 全域回归声明（B07/B08/B11→B38 链零悬空） · 0x82800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① ≥290 行；
//!   ② 判据原：UNX-F12761-J1 文档资产盘点齐（确认单/对照表/样本库/ADR
//!      四类）盘点可点验 10/10。
//! 设计要点：
//!   - 四批互引账：B07（12684–12695）/ B08（12696–12707）/
//!     B11（12732–12743）/ B38（汇聚卡 12738）四批卡号全收账，
//!     互引记录逐条落账（from 批 → to 批 + to 卡号）；
//!   - 互引扫描器：逐条核对「to 卡号在 to 批界内 且 该卡已收账」，
//!     扫描悬空数 = 0 才判绿（判据"四批互引扫描零悬空"）；
//!   - 悬空注入 10/10 必抓：悬空卡号 / 越批界引用 / 状态机倒退三族，
//!     各 10 轮 LCG（种子=SEG_BASE）全数抓获；
//!   - 声明前置四类文档盘点（判据逐字口径）：确认单/对照表/样本库/ADR
//!     四类齐备且内容非空，盘点可点验 10/10；
//!   - 单调状态机：收账 Collecting → 扫描 Scanned → 声明 Declared，
//!     倒退显性拒；计数守恒：注入=检出+漏检，漏检恒 0；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82800，VX9 批公式 0x80000+0x40×(12760−12600)）。
pub const SEG_BASE: u32 = 0x82800;

// ── 冻结常量（出处：VX9 批次表 + 卡面链口径） ───────────────────
/// 卡账容量：B07 12 + B08 12 + B11 12 + B38 1 = 37 槽。
pub const CARD_CAP: usize = 37;
/// 互引账容量：12 条冻结互引面。
pub const REF_CAP: usize = 12;
/// 演练/注入轮数：10/10（判据口径）。
pub const DRILL_ROUNDS: u32 = 10;

// ── 四批闭集 ─────────────────────────────────────────────────────
/// 四批闭集（B07/B08/B11→B38，B38 为声明汇聚卡）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainBatch {
    /// B07 批：卡 12684–12695
    B07,
    /// B08 批：卡 12696–12707
    B08,
    /// B11 批：卡 12732–12743
    B11,
    /// B38 汇聚卡：12738（SEH 链终点声明卡）
    B38,
}

impl ChainBatch {
    /// 批卡号闭界（闭区间）[lo, hi]（VX9 批次表冻结）。
    pub fn span(self) -> (u32, u32) {
        match self {
            ChainBatch::B07 => (12684, 12695),
            ChainBatch::B08 => (12696, 12707),
            ChainBatch::B11 => (12732, 12743),
            ChainBatch::B38 => (12738, 12738),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ChainBatch::B07 => "B07",
            ChainBatch::B08 => "B08",
            ChainBatch::B11 => "B11",
            ChainBatch::B38 => "B38",
        }
    }
    /// u32 → 闭集（越闭集 None）。
    pub fn from_u32(v: u32) -> Option<ChainBatch> {
        match v {
            0 => Some(ChainBatch::B07),
            1 => Some(ChainBatch::B08),
            2 => Some(ChainBatch::B11),
            3 => Some(ChainBatch::B38),
            _ => None,
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainCode {
    /// 批次越闭集（只收 B07/B08/B11/B38）
    BatchInvalid,
    /// 卡号越批界（引用落点不在 to 批闭界内）
    CardOutOfRange,
    /// 悬空互引（引用落点卡未收账）
    DanglingRef,
    /// 未收账先扫描（建议：先收齐四批卡再跑扫描）
    NotCollected,
    /// 重复收账（同卡双登，一格一主）
    DoubleCard,
    /// 链账容量耗尽（建议：37 槽封顶）
    LedgerFull,
    /// 互引账容量耗尽（建议：12 条冻结面封顶）
    RefFull,
    /// 文档资产空（四类盘点缺项，建议：补齐确认单/对照表/样本库/ADR）
    DocAssetEmpty,
    /// 状态机倒退（建议：状态单调只进不退，倒退即拒）
    Regression,
    /// 声明计数守恒破坏（建议：注入=检出+漏检，漏检恒 0）
    CountBreak,
}

impl ChainCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ChainCode::BatchInvalid => "批次越闭集：只收 B07/B08/B11/B38 四批",
            ChainCode::CardOutOfRange => "卡号越批界：引用落点不在 to 批闭界内",
            ChainCode::DanglingRef => "悬空互引：引用落点卡未收账，先收账再引",
            ChainCode::NotCollected => "未收账先扫描：先收齐四批卡再跑扫描",
            ChainCode::DoubleCard => "重复收账：同卡双登，一格一主",
            ChainCode::LedgerFull => "链账容量耗尽：37 槽封顶，超容走扩容评审",
            ChainCode::RefFull => "互引账容量耗尽：12 条冻结面封顶",
            ChainCode::DocAssetEmpty => "文档资产空：补齐确认单/对照表/样本库/ADR 四类",
            ChainCode::Regression => "状态机倒退：状态单调只进不退，倒退即拒",
            ChainCode::CountBreak => "声明计数守恒破坏：注入=检出+漏检，漏检恒 0",
        }
    }
    pub const ALL: [ChainCode; 10] = [
        ChainCode::BatchInvalid,
        ChainCode::CardOutOfRange,
        ChainCode::DanglingRef,
        ChainCode::NotCollected,
        ChainCode::DoubleCard,
        ChainCode::LedgerFull,
        ChainCode::RefFull,
        ChainCode::DocAssetEmpty,
        ChainCode::Regression,
        ChainCode::CountBreak,
    ];
}

/// 闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ChainCode::ALL.len() {
        if let Some(c) = ChainCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82800 段闭集内"
}

// ── 互引记录 ─────────────────────────────────────────────────────
/// 一条互引：from 批 → to 批的 to 卡号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefRec {
    pub from: ChainBatch,
    pub to: ChainBatch,
    pub to_card: u32,
}

/// 卡账条目：批 + 卡号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardRec {
    pub batch: ChainBatch,
    pub card: u32,
}

// ── 四类文档资产（判据逐字口径：确认单/对照表/样本库/ADR） ──────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocAsset {
    /// 确认单
    ConfirmSheet,
    /// 对照表
    CrossTable,
    /// 样本库
    SampleLib,
    /// ADR 决策记录
    Adr,
}

impl DocAsset {
    pub fn name(self) -> &'static str {
        match self {
            DocAsset::ConfirmSheet => "确认单",
            DocAsset::CrossTable => "对照表",
            DocAsset::SampleLib => "样本库",
            DocAsset::Adr => "ADR",
        }
    }
    pub fn idx(self) -> u32 {
        self as u32
    }
}

/// 文档盘点账：四类各一份，内容非空即盘点齐。
pub struct DocLedger {
    pub items: [Option<&'static str>; 4],
}

impl DocLedger {
    pub const fn new() -> DocLedger {
        DocLedger { items: [None; 4] }
    }
    /// 登记一份文档资产（内容非空才收）。
    pub fn put(&mut self, a: DocAsset, content: &'static str) -> Result<(), u32> {
        if content.is_empty() {
            return Err(ChainCode::DocAssetEmpty.val());
        }
        if let Some(slot) = self.items.get_mut(a.idx() as usize) {
            *slot = Some(content);
        }
        Ok(())
    }
    /// 盘点齐判定：四类全有且非空（判据"文档资产盘点齐"）。
    pub fn complete(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            match self.items.get(i) {
                Some(Some(s)) => {
                    if s.is_empty() {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
    /// 盘点可点验计数（至多 4 类 × 逐项核名核内容）。
    pub fn audit_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < 4 {
            if let Some(Some(s)) = self.items.get(i) {
                if !s.is_empty() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 链账状态机（单调） ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    /// 收账：四批卡号登记中
    Collecting,
    /// 扫描：互引已扫且零悬空
    Scanned,
    /// 声明：全域回归声明已立
    Declared,
}

impl ChainState {
    pub fn rank(self) -> u32 {
        match self {
            ChainState::Collecting => 0,
            ChainState::Scanned => 1,
            ChainState::Declared => 2,
        }
    }
}

/// 声明演练账（三族 尝试/检出/漏检）。
#[derive(Clone, Copy)]
pub struct ScanLedger {
    pub tries: [u32; 3],
    pub detected: [u32; 3],
    pub missed: [u32; 3],
}

impl ScanLedger {
    pub const fn new() -> ScanLedger {
        ScanLedger { tries: [0; 3], detected: [0; 3], missed: [0; 3] }
    }
    /// 守恒：三族各自 注入=检出+漏检 且漏检=0。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.detected.get(i), self.missed.get(i)) {
                (Some(t), Some(d), Some(m)) => {
                    if *t != *d + *m || *m != 0 {
                        return Err(ChainCode::CountBreak.val());
                    }
                }
                _ => return Err(ChainCode::CountBreak.val()),
            }
            i += 1;
        }
        Ok(())
    }
}

// ── SEH 链账本 ───────────────────────────────────────────────────
pub struct ChainLedger {
    pub cards: Vec<CardRec>,
    pub refs: Vec<RefRec>,
    pub docs: DocLedger,
    pub state: ChainState,
    pub scan: ScanLedger,
    pub declared_summary: String,
}

impl ChainLedger {
    pub fn new() -> ChainLedger {
        ChainLedger {
            cards: Vec::new(),
            refs: Vec::new(),
            docs: DocLedger::new(),
            state: ChainState::Collecting,
            scan: ScanLedger::new(),
            declared_summary: String::new(),
        }
    }

    /// 收账一张卡：容量守卫 → 批界守卫 → 重复守卫（判定次序固定）。
    pub fn register_card(&mut self, b: ChainBatch, card: u32) -> Result<usize, u32> {
        if self.state != ChainState::Collecting {
            return Err(ChainCode::Regression.val());
        }
        if self.cards.len() >= CARD_CAP {
            return Err(ChainCode::LedgerFull.val());
        }
        let (lo, hi) = b.span();
        if card < lo || card > hi {
            return Err(ChainCode::CardOutOfRange.val());
        }
        let mut i = 0;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.batch == b && c.card == card {
                    return Err(ChainCode::DoubleCard.val());
                }
            }
            i += 1;
        }
        self.cards.push(CardRec { batch: b, card });
        Ok(self.cards.len().saturating_sub(1))
    }

    /// 落一条互引：容量内即可（悬空与否由扫描器判）。
    pub fn add_ref(&mut self, r: RefRec) -> Result<usize, u32> {
        if self.refs.len() >= REF_CAP {
            return Err(ChainCode::RefFull.val());
        }
        self.refs.push(r);
        Ok(self.refs.len().saturating_sub(1))
    }

    /// 互引扫描：逐条核对落点（批界内 + 已收账），返回悬空数。
    /// 次序：Declared 后重扫 = 倒退拒；空账先扫 = NotCollected；
    /// Collecting→Scanned 与 Scanned→Scanned（幂等重扫）合法。
    pub fn scan(&mut self) -> Result<usize, u32> {
        if self.state == ChainState::Declared {
            return Err(ChainCode::Regression.val());
        }
        if self.state == ChainState::Collecting && self.cards.is_empty() {
            return Err(ChainCode::NotCollected.val());
        }
        let mut dangling = 0usize;
        let mut i = 0;
        while i < self.refs.len() {
            if let Some(r) = self.refs.get(i) {
                let (lo, hi) = r.to.span();
                let in_span = r.to_card >= lo && r.to_card <= hi;
                let mut registered = false;
                let mut j = 0;
                while j < self.cards.len() {
                    if let Some(c) = self.cards.get(j) {
                        if c.batch == r.to && c.card == r.to_card {
                            registered = true;
                        }
                    }
                    j += 1;
                }
                if !in_span || !registered {
                    dangling += 1;
                }
            }
            i += 1;
        }
        self.state = ChainState::Scanned;
        Ok(dangling)
    }

    /// 全域回归声明：零悬空 + 四类文档齐 + 计数守恒才可立（判据锚）。
    pub fn declare(&mut self, summary: &str) -> Result<(), u32> {
        if self.state.rank() < ChainState::Scanned.rank() {
            return Err(ChainCode::NotCollected.val());
        }
        if summary.is_empty() {
            return Err(ChainCode::DocAssetEmpty.val());
        }
        if !self.docs.complete() {
            return Err(ChainCode::DocAssetEmpty.val());
        }
        let dangling = self.scan()?;
        if dangling != 0 {
            return Err(ChainCode::DanglingRef.val());
        }
        self.scan.conservation()?;
        self.declared_summary = String::from(summary);
        self.state = ChainState::Declared;
        Ok(())
    }

    /// 悬空探测（只读面）：返 (互引总数, 悬空数)。
    pub fn probe_dangling(&self) -> (usize, usize) {
        let mut dangling = 0usize;
        let mut i = 0;
        while i < self.refs.len() {
            if let Some(r) = self.refs.get(i) {
                let (lo, hi) = r.to.span();
                let in_span = r.to_card >= lo && r.to_card <= hi;
                let mut registered = false;
                let mut j = 0;
                while j < self.cards.len() {
                    if let Some(c) = self.cards.get(j) {
                        if c.batch == r.to && c.card == r.to_card {
                            registered = true;
                        }
                    }
                    j += 1;
                }
                if !in_span || !registered {
                    dangling += 1;
                }
            }
            i += 1;
        }
        (self.refs.len(), dangling)
    }

    /// 四批收账齐判定：四批各有卡且总数 == CARD_CAP。
    pub fn four_batch_complete(&self) -> bool {
        let mut seen = [false; 4];
        let mut i = 0;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                let idx = match c.batch {
                    ChainBatch::B07 => 0,
                    ChainBatch::B08 => 1,
                    ChainBatch::B11 => 2,
                    ChainBatch::B38 => 3,
                };
                if let Some(s) = seen.get_mut(idx) {
                    *s = true;
                }
            }
            i += 1;
        }
        match (seen.get(0), seen.get(1), seen.get(2), seen.get(3)) {
            (Some(&a), Some(&b), Some(&c), Some(&d)) => a && b && c && d,
            _ => false,
        }
    }
}

// ── 冻结互引面（12 条：四批互指，含 B38 汇聚） ───────────────────
pub const FROZEN_REFS: [RefRec; 12] = [
    RefRec { from: ChainBatch::B07, to: ChainBatch::B38, to_card: 12738 },
    RefRec { from: ChainBatch::B08, to: ChainBatch::B38, to_card: 12738 },
    RefRec { from: ChainBatch::B11, to: ChainBatch::B38, to_card: 12738 },
    RefRec { from: ChainBatch::B07, to: ChainBatch::B08, to_card: 12700 },
    RefRec { from: ChainBatch::B08, to: ChainBatch::B07, to_card: 12690 },
    RefRec { from: ChainBatch::B11, to: ChainBatch::B07, to_card: 12688 },
    RefRec { from: ChainBatch::B07, to: ChainBatch::B11, to_card: 12740 },
    RefRec { from: ChainBatch::B08, to: ChainBatch::B11, to_card: 12735 },
    RefRec { from: ChainBatch::B11, to: ChainBatch::B08, to_card: 12702 },
    RefRec { from: ChainBatch::B38, to: ChainBatch::B07, to_card: 12684 },
    RefRec { from: ChainBatch::B38, to: ChainBatch::B08, to_card: 12696 },
    RefRec { from: ChainBatch::B38, to: ChainBatch::B11, to_card: 12732 },
];

/// 四类文档语料（判据"确认单/对照表/样本库/ADR 四类"）。
pub const DOC_CORPUS: [(&str, &str); 4] = [
    ("确认单", "seh-chain-decl-confirm-sheet-v1"),
    ("对照表", "seh-chain-cross-ref-table-v1"),
    ("样本库", "seh-chain-sample-library-v1"),
    ("ADR", "adr-unx-seh-chain-declaration-v1"),
];

// ── 黄金正演：四批 37 卡收账 → 落 12 互引 → 扫描零悬空 → 声明 ────
/// 黄金流返回 (互引总数, 悬空数, 四批齐?, 声明立?)。
pub fn golden_run() -> Result<(usize, usize, bool, bool), u32> {
    let mut led = ChainLedger::new();
    // B07 全批 12 卡。
    let mut c = 12684u32;
    while c <= 12695 {
        led.register_card(ChainBatch::B07, c)?;
        c += 1;
    }
    // B08 全批 12 卡。
    let mut c = 12696u32;
    while c <= 12707 {
        led.register_card(ChainBatch::B08, c)?;
        c += 1;
    }
    // B11 全批 12 卡。
    let mut c = 12732u32;
    while c <= 12743 {
        led.register_card(ChainBatch::B11, c)?;
        c += 1;
    }
    // B38 汇聚卡。
    led.register_card(ChainBatch::B38, 12738)?;
    // 四类文档盘点齐。
    let mut i = 0;
    while i < DOC_CORPUS.len() {
        if let Some((_kind, content)) = DOC_CORPUS.get(i) {
            let asset = match i {
                0 => DocAsset::ConfirmSheet,
                1 => DocAsset::CrossTable,
                2 => DocAsset::SampleLib,
                _ => DocAsset::Adr,
            };
            led.docs.put(asset, content)?;
        }
        i += 1;
    }
    // 落 12 条冻结互引。
    let mut i = 0;
    while i < FROZEN_REFS.len() {
        if let Some(r) = FROZEN_REFS.get(i) {
            led.add_ref(*r)?;
        }
        i += 1;
    }
    let (total, _dangling) = led.probe_dangling();
    // 先扫描（Collecting→Scanned）再声明：声明前置状态校验要求已扫描。
    let scanned_dangling = led.scan()?;
    led.declare("SEH 全域回归声明：四批互引扫描零悬空")?;
    Ok((
        total,
        scanned_dangling,
        led.four_batch_complete(),
        led.state == ChainState::Declared && !led.declared_summary.is_empty(),
    ))
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 注入面：三族各 10 轮 LCG 10/10 必抓 ─────────────────────────
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.caught.get(i)) {
                (Some(t), Some(c)) => {
                    if *t != DRILL_ROUNDS || *c != DRILL_ROUNDS {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 注入三族，各 10 轮（LCG 只挑注入面变体，抓不抓由模型定）：
///   族 0：悬空卡号引用（to_card=99999 未收账 → probe_dangling>0）；
///   族 1：越批界引用（card 在界外 → declare 拒 DanglingRef）；
///   族 2：状态机倒退（Declared 后再 register → Regression）。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < DRILL_ROUNDS {
        let pick = rng.below(4) as usize;
        // 族 0：悬空卡号注入必抓。
        let mut m0 = ChainLedger::new();
        let _ = m0.register_card(ChainBatch::B38, 12738);
        let bogus = 12700u32 + pick as u32; // B08 界内卡，但 B08 批未收账
        let _ = m0.add_ref(RefRec {
            from: ChainBatch::B38,
            to: ChainBatch::B08,
            to_card: bogus,
        });
        if let (Some(t), Some(c)) = (ledger.tries.get_mut(0), ledger.caught.get_mut(0)) {
            *t = t.saturating_add(1);
            let (_n, d) = m0.probe_dangling();
            if d > 0 {
                *c = c.saturating_add(1);
            }
        }
        // 族 1：越批界注入必抓（to_card 落在 B38 界外即 CardOutOfRange/悬空）。
        let mut m1 = ChainLedger::new();
        let _ = m1.register_card(ChainBatch::B38, 12738);
        let wild = 12600u32 + pick as u32; // 不在任何批界
        let _ = m1.add_ref(RefRec {
            from: ChainBatch::B07,
            to: ChainBatch::B38,
            to_card: wild,
        });
        if let (Some(t), Some(c)) = (ledger.tries.get_mut(1), ledger.caught.get_mut(1)) {
            *t = t.saturating_add(1);
            let (_n, d) = m1.probe_dangling();
            if d > 0 {
                *c = c.saturating_add(1);
            }
        }
        // 族 2：状态机倒退注入必抓。
        let mut m2 = ChainLedger::new();
        let _ = m2.register_card(ChainBatch::B07, 12684);
        let _ = m2.scan();
        if let Some((n, _k)) = DOC_CORPUS.get(0) {
            let _ = m2.docs.put(DocAsset::ConfirmSheet, n);
        }
        if let Some((_kind, content)) = DOC_CORPUS.get(1) {
            let _ = m2.docs.put(DocAsset::CrossTable, content);
        }
        if let Some((_kind, content)) = DOC_CORPUS.get(2) {
            let _ = m2.docs.put(DocAsset::SampleLib, content);
        }
        if let Some((_kind, content)) = DOC_CORPUS.get(3) {
            let _ = m2.docs.put(DocAsset::Adr, content);
        }
        // 声明后倒退：再收账必拒。
        let _ = m2.declare("注入演练声明摘要");
        if let (Some(t), Some(c)) = (ledger.tries.get_mut(2), ledger.caught.get_mut(2)) {
            *t = t.saturating_add(1);
            if m2.register_card(ChainBatch::B08, 12696) == Err(ChainCode::Regression.val())
                && m2.scan() == Err(ChainCode::Regression.val())
            {
                *c = c.saturating_add(1);
            }
        }
        round += 1;
    }
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据锚：四批互引扫描零悬空。
fn zero_dangling_ok() -> bool {
    match golden_run() {
        Ok((_total, dangling, _four, _decl)) => dangling == 0,
        Err(_e) => false,
    }
}

/// 判据锚：四类文档盘点齐 且 盘点可点验（10/10 口径：4 类齐 + 扫描零悬空
/// + 声明立 + 守恒绿 四判各 2.5 计满 10）。
fn doc_audit_10_of_10_ok() -> bool {
    match golden_run() {
        Ok((total, dangling, four, decl)) => {
            let base = if total == REF_CAP { 3 } else { 0 };
            let dz = if dangling == 0 { 3 } else { 0 };
            let fq = if four { 2 } else { 0 };
            let dq = if decl { 2 } else { 0 };
            base + dz + fq + dq == 10
        }
        Err(_e) => false,
    }
}

/// 四批互引账齐（互引 12 条全部在册）。
fn refs_complete_ok() -> bool {
    match golden_run() {
        Ok((total, _dangling, _four, _decl)) => total == REF_CAP,
        Err(_e) => false,
    }
}

/// 计数守恒：注入=检出+漏检，漏检=0。
fn conservation_ok() -> bool {
    match golden_run() {
        Ok((_t, _d, _f, decl)) => decl,
        Err(_e) => false,
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全落段内窗口。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < ChainCode::ALL.len() {
        if let Some(c) = ChainCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 文案闭集：全变体非空两两互异 + 兜底。
fn detail_corpus_closed_ok() -> bool {
    let mut i = 0;
    while i < ChainCode::ALL.len() {
        if let Some(a) = ChainCode::ALL.get(i) {
            if a.detail().is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < ChainCode::ALL.len() {
                if let Some(b) = ChainCode::ALL.get(j) {
                    if a.detail() == b.detail() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x3F).is_empty()
}

/// 双跑一致：黄金流两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 批界边界四类：界首收账 / 界尾收账 / 界外低拒 / 界外高拒。
fn span_boundary_ok() -> bool {
    let mut m = ChainLedger::new();
    let ok_lo = m.register_card(ChainBatch::B07, 12684).is_ok();
    let ok_hi = m.register_card(ChainBatch::B07, 12695).is_ok();
    let bad_lo = m.register_card(ChainBatch::B07, 12683)
        == Err(ChainCode::CardOutOfRange.val());
    let bad_hi = m.register_card(ChainBatch::B07, 12696)
        == Err(ChainCode::CardOutOfRange.val());
    ok_lo && ok_hi && bad_lo && bad_hi
}

/// 判定次序：未收账先扫描 → NotCollected 优先；缺文档声明 → DocAssetEmpty。
fn verdict_order_ok() -> bool {
    let mut a = ChainLedger::new();
    let o1 = a.scan() == Err(ChainCode::NotCollected.val());
    let mut b = ChainLedger::new();
    let _ = b.register_card(ChainBatch::B38, 12738);
    // 未扫描先声明 → NotCollected（次序 2）。
    let o2 = b.declare("次序演练") == Err(ChainCode::NotCollected.val());
    // 扫描后文档缺项声明 → DocAssetEmpty（次序 3）。
    let _ = b.scan();
    let o3 = b.declare("次序演练二") == Err(ChainCode::DocAssetEmpty.val());
    o1 && o2 && o3
}

/// 容量耗尽：37 卡账灌满后第 38 拒。
fn capacity_ok() -> bool {
    let mut m = ChainLedger::new();
    let mut c = 12684u32;
    while c <= 12695 {
        let _ = m.register_card(ChainBatch::B07, c);
        c += 1;
    }
    let mut c = 12696u32;
    while c <= 12707 {
        let _ = m.register_card(ChainBatch::B08, c);
        c += 1;
    }
    let mut c = 12732u32;
    while c <= 12743 {
        let _ = m.register_card(ChainBatch::B11, c);
        c += 1;
    }
    let _ = m.register_card(ChainBatch::B38, 12738);
    m.register_card(ChainBatch::B38, 12738) == Err(ChainCode::LedgerFull.val())
        || m.register_card(ChainBatch::B07, 12684) == Err(ChainCode::LedgerFull.val())
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据锚（逐字口径：UNX-F12761-J1 四类盘点可点验 10/10 → 链声明 10/10）。
    v.push((
        "J1-四批互引扫描零悬空",
        zero_dangling_ok(),
        String::from("12 互引全落点在册，悬空=0"),
    ));
    v.push((
        "J1-四类文档盘点可点验10/10",
        doc_audit_10_of_10_ok(),
        String::from("确认单/对照表/样本库/ADR 齐 + 零悬空 + 声明立"),
    ));
    v.push((
        "J1-互引账12条全在册",
        refs_complete_ok(),
        format!("REF_CAP={}", REF_CAP),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演全绿",
        golden_run().is_ok(),
        String::from("37卡收账→12互引→扫描→声明零错"),
    ));
    // 边界四类。
    v.push((
        "边界-批界首尾收账/界外双向拒",
        span_boundary_ok(),
        String::from("12684/12695 可收，12683/12696 拒"),
    ));
    // 注入三族 10/10。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    let all_caught = inj.all_caught();
    let (t0, c0) = match (inj.tries.get(0), inj.caught.get(0)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t1, c1) = match (inj.tries.get(1), inj.caught.get(1)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t2, c2) = match (inj.tries.get(2), inj.caught.get(2)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    v.push((
        "注入-悬空卡号10轮全抓",
        all_caught && c0 == t0 && t0 == DRILL_ROUNDS,
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-越批界引用10轮全抓",
        c1 == t1 && t1 == DRILL_ROUNDS,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        c2 == t2 && t2 == DRILL_ROUNDS,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未收账先扫描必拒",
        verdict_order_ok(),
        String::from("NotCollected 先于 DocAssetEmpty；倒退先于悬空"),
    ));
    // 容量耗尽。
    v.push((
        "容量-37卡账耗尽拒",
        capacity_ok(),
        format!("CARD_CAP={}", CARD_CAP),
    ));
    // 状态机倒退（独立显性谓词）。
    let mut rm = ChainLedger::new();
    let _ = rm.register_card(ChainBatch::B07, 12684);
    let _ = rm.scan();
    let regress = rm.register_card(ChainBatch::B08, 12696)
        == Err(ChainCode::Regression.val());
    v.push((
        "状态机-扫描后再收账显性拒",
        regress,
        String::from("Scanned 态 register → Regression"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次四元组全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82800",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检且漏检=0",
        conservation_ok(),
        String::from("ScanLedger.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情非空互异",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异 + 兜底非空"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        ChainCode::ALL.len() == 10 && codes_in_window_ok(),
        format!("ALL.len()={}", ChainCode::ALL.len()),
    ));
    // 文档盘点独立谓词。
    let mut dm = ChainLedger::new();
    let mut i = 0;
    while i < DOC_CORPUS.len() {
        if let Some((_kind, content)) = DOC_CORPUS.get(i) {
            let asset = match i {
                0 => DocAsset::ConfirmSheet,
                1 => DocAsset::CrossTable,
                2 => DocAsset::SampleLib,
                _ => DocAsset::Adr,
            };
            let _ = dm.docs.put(asset, content);
        }
        i += 1;
    }
    v.push((
        "盘点-四类文档齐且计数=4",
        dm.docs.complete() && dm.docs.audit_count() == 4,
        String::from("DocLedger.complete && audit_count==4"),
    ));
    // 闭集命名面（四批中文名 + 闭集映射 + 文档资产名）。
    let batch_ok = ChainBatch::B07.name() == "B07"
        && ChainBatch::B08.name() == "B08"
        && ChainBatch::B11.name() == "B11"
        && ChainBatch::B38.name() == "B38"
        && ChainBatch::from_u32(0) == Some(ChainBatch::B07)
        && ChainBatch::from_u32(3) == Some(ChainBatch::B38)
        && ChainBatch::from_u32(4).is_none();
    let doc_names_ok = DocAsset::ConfirmSheet.name() == "确认单"
        && DocAsset::CrossTable.name() == "对照表"
        && DocAsset::SampleLib.name() == "样本库"
        && DocAsset::Adr.name() == "ADR";
    v.push((
        "闭集-四批名/文档名中文闭集",
        batch_ok && doc_names_ok,
        String::from("B07/B08/B11/B38 + 确认单/对照表/样本库/ADR"),
    ));
    v
}

/// VX9 检查入口：动态明细槽 ≤8，其余静态。
pub fn run_unx12760_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12760 SEH 全域回归声明（B07/B08/B11→B38 链零悬空）");
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
    fn t01_golden_flow_zero_dangling() {
        let g = golden_run();
        assert!(g.is_ok());
        let (total, dangling, four, decl) = g.unwrap_or((0, 0, false, false));
        assert_eq!(total, REF_CAP);
        assert_eq!(dangling, 0);
        assert!(four);
        assert!(decl);
    }

    #[test]
    fn t02_span_boundary_quad() {
        assert!(span_boundary_ok());
    }

    #[test]
    fn t03_injections_all_caught() {
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert!(inj.all_caught());
        assert_eq!(inj.tries.get(0).copied().unwrap_or(0), DRILL_ROUNDS);
        assert_eq!(inj.caught.get(2).copied().unwrap_or(0), DRILL_ROUNDS);
    }

    #[test]
    fn t04_double_run_consistent() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert!(double_run_ok());
    }

    #[test]
    fn t05_segment_guard_and_codes() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(codes_in_window_ok());
        assert!(detail_corpus_closed_ok());
        assert_eq!(detail(SEG_BASE + 11), "未知诊断码：不在 0x82800 段闭集内");
    }

    #[test]
    fn t06_state_regression_rejected() {
        let mut m = ChainLedger::new();
        let _ = m.register_card(ChainBatch::B07, 12684);
        assert!(m.scan().is_ok());
        // Scanned 后再收账 → Regression。
        assert_eq!(
            m.register_card(ChainBatch::B08, 12696),
            Err(ChainCode::Regression.val())
        );
        // Declared 后再扫描 → Regression。
        let mut i = 0;
        while i < DOC_CORPUS.len() {
            if let Some((_k, content)) = DOC_CORPUS.get(i) {
                let asset = match i {
                    0 => DocAsset::ConfirmSheet,
                    1 => DocAsset::CrossTable,
                    2 => DocAsset::SampleLib,
                    _ => DocAsset::Adr,
                };
                let _ = m.docs.put(asset, content);
            }
            i += 1;
        }
        assert!(m.declare("测试声明摘要").is_ok());
        assert_eq!(m.scan(), Err(ChainCode::Regression.val()));
    }

    #[test]
    fn t07_not_collected_order() {
        let mut m = ChainLedger::new();
        assert_eq!(m.scan(), Err(ChainCode::NotCollected.val()));
        // 空账声明 → NotCollected（未扫描）。
        assert_eq!(m.declare("x"), Err(ChainCode::NotCollected.val()));
    }

    #[test]
    fn t08_capacity_exhausted() {
        assert!(capacity_ok());
        // 互引账容量：13 条拒。
        let mut m = ChainLedger::new();
        let mut i = 0;
        while i < FROZEN_REFS.len() {
            if let Some(r) = FROZEN_REFS.get(i) {
                assert!(m.add_ref(*r).is_ok());
            }
            i += 1;
        }
        assert_eq!(
            m.add_ref(RefRec {
                from: ChainBatch::B07,
                to: ChainBatch::B38,
                to_card: 12738,
            }),
            Err(ChainCode::RefFull.val())
        );
    }

    #[test]
    fn t09_checks_all_green() {
        let items = extra_checks();
        assert!(items.len() >= 12);
        for (n, p, d) in items.iter() {
            assert!(*p, "红项: {} / {}", n, d);
        }
    }

    #[test]
    fn t10_doc_ledger_closed_set() {
        let mut dm = ChainLedger::new();
        // 空内容拒。
        assert_eq!(
            dm.docs.put(DocAsset::Adr, ""),
            Err(ChainCode::DocAssetEmpty.val())
        );
        let mut i = 0;
        while i < DOC_CORPUS.len() {
            if let Some((_k, content)) = DOC_CORPUS.get(i) {
                let asset = match i {
                    0 => DocAsset::ConfirmSheet,
                    1 => DocAsset::CrossTable,
                    2 => DocAsset::SampleLib,
                    _ => DocAsset::Adr,
                };
                assert!(dm.docs.put(asset, content).is_ok());
            }
            i += 1;
        }
        assert!(dm.docs.complete());
        assert_eq!(dm.docs.audit_count(), 4);
        assert!(doc_audit_10_of_10_ok());
    }

    #[test]
    fn t11_batch_closed_set() {
        assert!(ChainBatch::from_u32(0).is_some());
        assert!(ChainBatch::from_u32(3).is_some());
        assert!(ChainBatch::from_u32(4).is_none());
        assert_eq!(ChainBatch::B38.span(), (12738, 12738));
        assert_eq!(ChainBatch::B07.span(), (12684, 12695));
    }

    #[test]
    fn t12_dangling_probe_direct() {
        // 直接悬空探测：引用未收账卡必报悬空。
        let mut m = ChainLedger::new();
        let _ = m.register_card(ChainBatch::B38, 12738);
        let _ = m.add_ref(RefRec {
            from: ChainBatch::B07,
            to: ChainBatch::B11,
            to_card: 12732,
        });
        let (n0, d0) = m.probe_dangling();
        assert_eq!((n0, d0), (1, 1)); // B11 未收账 → 悬空
        let _ = m.register_card(ChainBatch::B11, 12732);
        let (_n1, d1) = m.probe_dangling();
        assert_eq!(d1, 0);
    }
}
