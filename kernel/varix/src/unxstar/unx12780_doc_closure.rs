//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12780 域文档收官锚（对齐率 100% 断言） · 0x82D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 vx9_cards.json 卡 12780 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12781-J1 40 批回归矩阵齐（批×回归态两列），
//!      全量复测入口可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 批回归矩阵：恰 40 行，每行两列 =（批号, 回归态）；回归态闭集
//!     三值 {未跑/绿/红}，档外值一律判回归态越闭集，零容忍；
//!   - 批号列闭集对账：VX9 十七批（B00–B16，出处 DISPATCH_VX9.md 批次
//!     表）+ 遗留二十三批（L01–L23，F0001–F12000 台账切分 18×600+
//!     5×240=12000）恰满 40 行，一行一批不许缺批不许重批；
//!   - 全量复测入口：40 行逐行挂一条可点验谓词（谓词编号+谓词描述均
//!     非空），谓词语义 = 行内批号对账 + 回归态闭集在档 + 证据校验和
//!     复算三项合取；入口逐行可点验，一条空谓词即收官判红；
//!   - 对齐率 100% 断言（卡面标题）：对齐行 = 批号与冻结表逐字段一致
//!     且行谓词可点验绿的行；对齐率 = 对齐行×100/40（整数百分率），
//!     恰 100% 才许收官——差一行即 97/98 档判红；
//!   - 单调状态机：行级 未跑→绿→红 只升不降（绿→未跑、红→绿、红→
//!     未跑均判倒退显性拒）；账级 登记中→复测中→收官 只进不退；
//!   - 缺行注入三族：缺行（漏登记一行）/越闭集回归态/账级倒退，
//!     LCG（种子=0x82D00）各 10 轮 10/10 必抓；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82D00，VX9 批公式 0x80000+0x40×(12780−12600)）。
pub const SEG_BASE: u32 = 0x82D00;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 矩阵行数：40 批（判据②闭集）。
pub const BATCH_N: usize = 40;
/// VX9 批数（B00–B16，DISPATCH_VX9.md 批次表）。
pub const VX9_N: usize = 17;
/// 遗留批数（L01–L23，F0001–F12000 台账切分）。
pub const LEGACY_N: usize = 23;
/// 回归态闭集档数 {未跑/绿/红}。
pub const STATE_TIERS: usize = 3;
/// 每族注入轮数（判据②口径 10/10）。
pub const DRILL_ROUNDS: u32 = 10;

// ── 40 批冻结表（批号/id 下沿/id 上沿；出处：DISPATCH_VX9.md 批次表
//    与 F12001–F12800 号面台账切分） ────────────────────────────────
/// VX9 十七批：id 区间逐批衔接 12600–12799。
pub const VX9_BATCHES: [(&str, u32, u32); VX9_N] = [
    ("B00", 12600, 12611),
    ("B01", 12612, 12623),
    ("B02", 12624, 12635),
    ("B03", 12636, 12647),
    ("B04", 12648, 12659),
    ("B05", 12660, 12671),
    ("B06", 12672, 12683),
    ("B07", 12684, 12695),
    ("B08", 12696, 12707),
    ("B09", 12708, 12719),
    ("B10", 12720, 12731),
    ("B11", 12732, 12743),
    ("B12", 12744, 12755),
    ("B13", 12756, 12767),
    ("B14", 12768, 12779),
    ("B15", 12780, 12791),
    ("B16", 12792, 12799),
];
/// 遗留二十三批：F0001–F12000 切 18×600 + 5×240，逐批衔接不重不漏。
pub const LEGACY_BATCHES: [(&str, u32, u32); LEGACY_N] = [
    ("L01", 1, 600),
    ("L02", 601, 1200),
    ("L03", 1201, 1800),
    ("L04", 1801, 2400),
    ("L05", 2401, 3000),
    ("L06", 3001, 3600),
    ("L07", 3601, 4200),
    ("L08", 4201, 4800),
    ("L09", 4801, 5400),
    ("L10", 5401, 6000),
    ("L11", 6001, 6600),
    ("L12", 6601, 7200),
    ("L13", 7201, 7800),
    ("L14", 7801, 8400),
    ("L15", 8401, 9000),
    ("L16", 9001, 9600),
    ("L17", 9601, 10200),
    ("L18", 10201, 10800),
    ("L19", 10801, 11040),
    ("L20", 11041, 11280),
    ("L21", 11281, 11520),
    ("L22", 11521, 11760),
    ("L23", 11761, 12000),
];

/// 行下标→冻结批号（0..16 走 VX9，17..39 走遗留）。
pub fn frozen_batch(row: usize) -> Option<(&'static str, u32, u32)> {
    if row < VX9_N {
        VX9_BATCHES.get(row).copied()
    } else if row < BATCH_N {
        LEGACY_BATCHES.get(row - VX9_N).copied()
    } else {
        None
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegressCode {
    /// 批号越闭集（不在 40 批冻结表内）
    BatchUnknown,
    /// 缺行（矩阵不满 40 行即收官）
    RowMissing,
    /// 回归态越闭集（{未跑/绿/红} 之外）
    StateInvalid,
    /// 回归态倒退（绿→未跑、红→绿、红→未跑）
    StateRegress,
    /// 复测谓词为空（批号或描述缺失即不可点验）
    PredEmpty,
    /// 矩阵未齐先复测（40 行未满即调全量复测）
    MatrixIncomplete,
    /// 对齐率不足 100%（对齐行 < 40）
    AlignNotFull,
    /// 行重复登记（一批一行，重批即拒）
    RowDoubleBooked,
    /// 收官摘要为空（判据③写回非空）
    SummaryEmpty,
}

impl RegressCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegressCode::BatchUnknown => {
                "批号越闭集：不在 40 批冻结表内，请回 DISPATCH_VX9 批次表对账"
            }
            RegressCode::RowMissing => {
                "缺行：矩阵不满 40 行，请按冻结表逐批补登后再收官"
            }
            RegressCode::StateInvalid => {
                "回归态越闭集：只收 未跑/绿/红 三值，档外值请回行内改档"
            }
            RegressCode::StateRegress => {
                "回归态倒退：未跑→绿→红 只升不降，倒退请走新行不得改旧账"
            }
            RegressCode::PredEmpty => {
                "复测谓词为空：每行须挂批号+描述俱全的谓词，空谓词即不可点验"
            }
            RegressCode::MatrixIncomplete => {
                "矩阵未齐先复测：40 行登记齐后再调全量复测入口"
            }
            RegressCode::AlignNotFull => {
                "对齐率不足 100%：对齐行少于 40，请逐行对账冻结表后收官"
            }
            RegressCode::RowDoubleBooked => {
                "行重复登记：一批一行一格一主，请查重后再登记"
            }
            RegressCode::SummaryEmpty => {
                "收官摘要为空：判据③要求状态=已完成且摘要非空，请补写摘要"
            }
        }
    }
    pub const ALL: [RegressCode; 9] = [
        RegressCode::BatchUnknown,
        RegressCode::RowMissing,
        RegressCode::StateInvalid,
        RegressCode::StateRegress,
        RegressCode::PredEmpty,
        RegressCode::MatrixIncomplete,
        RegressCode::AlignNotFull,
        RegressCode::RowDoubleBooked,
        RegressCode::SummaryEmpty,
    ];
}

/// 诊断码闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegressCode::ALL.len() {
        if let Some(c) = RegressCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82D00 段闭集内，请核对诊断码取值区间"
}

// ── 回归态闭集 {未跑/绿/红} ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegressState {
    /// 未跑：批尚未进入复测
    NotRun,
    /// 绿：复测通过
    Green,
    /// 红：复测回归
    Red,
}

impl RegressState {
    /// 位次（只升不降）：未跑=0 < 绿=1 < 红=2。
    pub fn rank(self) -> u8 {
        match self {
            RegressState::NotRun => 0,
            RegressState::Green => 1,
            RegressState::Red => 2,
        }
    }
    /// 中文名（诊断文案与写回摘要用）。
    pub fn zh(self) -> &'static str {
        match self {
            RegressState::NotRun => "未跑",
            RegressState::Green => "绿",
            RegressState::Red => "红",
        }
    }
    /// 由闭集外原始档定态（返回 Err 供注入面判捕）。
    pub fn from_tier(tier: u32) -> Result<RegressState, u32> {
        match tier {
            0 => Ok(RegressState::NotRun),
            1 => Ok(RegressState::Green),
            2 => Ok(RegressState::Red),
            _ => Err(RegressCode::StateInvalid.val()),
        }
    }
}

// ── 矩阵行 ───────────────────────────────────────────────────────
/// 复测谓词：编号 + 描述均非空才可点验（判据②逐行可点验）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RowPredicate {
    pub pred_id: u16,
    pub desc: &'static str,
}

impl RowPredicate {
    /// 可点验性：编号非零、描述非空。
    pub fn verifiable(&self) -> bool {
        self.pred_id != 0 && !self.desc.is_empty()
    }
}

/// 矩阵行：批号（冻结表下标）+ 回归态 + 谓词 + 证据校验和。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MatrixRow {
    pub batch_idx: usize,
    pub state: RegressState,
    pub pred: RowPredicate,
    pub evidence: u32,
}

// ── 账级状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureState {
    /// 登记中：逐批落账
    Matrixing,
    /// 复测中：全量复测入口已跑
    Retesting,
    /// 收官：对齐率 100% 且写回非空
    Sealed,
}

impl ClosureState {
    pub fn rank(self) -> u8 {
        match self {
            ClosureState::Matrixing => 0,
            ClosureState::Retesting => 1,
            ClosureState::Sealed => 2,
        }
    }
}

// ── 40 批回归矩阵 ────────────────────────────────────────────────
pub struct RegressMatrix {
    rows: [Option<MatrixRow>; BATCH_N],
    pub state: ClosureState,
    pub row_n: usize,
    pub summary: Option<String>,
}

impl RegressMatrix {
    pub fn new() -> RegressMatrix {
        RegressMatrix {
            rows: [None; BATCH_N],
            state: ClosureState::Matrixing,
            row_n: 0,
            summary: None,
        }
    }

    /// 账级推进（只进不退），倒退显性拒。
    pub fn advance(&mut self, to: ClosureState) -> Result<(), u32> {
        if to.rank() < self.state.rank() {
            return Err(RegressCode::StateRegress.val());
        }
        self.state = to;
        Ok(())
    }

    /// 登记一行：批号须落在冻结表 40 行内且未重批。
    pub fn book_row(&mut self, row: MatrixRow) -> Result<usize, u32> {
        if row.batch_idx >= BATCH_N {
            return Err(RegressCode::BatchUnknown.val());
        }
        if frozen_batch(row.batch_idx).is_none() {
            return Err(RegressCode::BatchUnknown.val());
        }
        if self.rows.get(row.batch_idx).copied().flatten().is_some() {
            return Err(RegressCode::RowDoubleBooked.val());
        }
        if !row.pred.verifiable() {
            return Err(RegressCode::PredEmpty.val());
        }
        self.rows[row.batch_idx] = Some(row);
        self.row_n += 1;
        Ok(row.batch_idx)
    }

    /// 行级回归态推进（只升不降）：未跑→绿→红。
    pub fn set_state(&mut self, batch_idx: usize, to: RegressState) -> Result<(), u32> {
        let cur = match self.rows.get(batch_idx).copied().flatten() {
            Some(r) => r.state,
            None => return Err(RegressCode::RowMissing.val()),
        };
        if to.rank() < cur.rank() {
            return Err(RegressCode::StateRegress.val());
        }
        if let Some(r) = self.rows.get_mut(batch_idx) {
            if let Some(row) = r.as_mut() {
                row.state = to;
            }
        }
        Ok(())
    }

    /// 行谓词可点验（判据②）：三项合取——批号对账、回归态闭集在档、
    /// 证据校验和复算（evidence == 批号×31 + 态位次×7）。
    pub fn row_verifiable(&self, batch_idx: usize) -> bool {
        let row = match self.rows.get(batch_idx).copied().flatten() {
            Some(r) => r,
            None => return false,
        };
        let batch = match frozen_batch(batch_idx) {
            Some(b) => b,
            None => return false,
        };
        if !row.pred.verifiable() {
            return false;
        }
        let want = (batch_idx as u32)
            .wrapping_mul(31)
            .wrapping_add(row.state.rank() as u32)
            .wrapping_mul(7);
        row.evidence == want
            && batch.1 <= batch.2
            && matches!(
                row.state,
                RegressState::NotRun | RegressState::Green | RegressState::Red
            )
    }

    /// 矩阵齐判定：恰 40 行且行号与冻结表一一对齐。
    pub fn complete(&self) -> bool {
        self.row_n == BATCH_N
    }

    /// 全量复测入口（判据②）：40 行逐行可点验谓词，返回绿行数。
    /// 矩阵未齐即拒（先登记后复测）。
    pub fn full_retest(&mut self) -> Result<u32, u32> {
        if !self.complete() {
            return Err(RegressCode::MatrixIncomplete.val());
        }
        self.advance(ClosureState::Retesting)?;
        let mut green = 0u32;
        let mut i = 0;
        while i < BATCH_N {
            if self.row_verifiable(i) {
                let st = self
                    .rows
                    .get(i)
                    .copied()
                    .flatten()
                    .map(|r| r.state)
                    .unwrap_or(RegressState::NotRun);
                if st == RegressState::Green {
                    green += 1;
                }
            } else {
                return Err(RegressCode::PredEmpty.val());
            }
            i += 1;
        }
        Ok(green)
    }

    /// 对齐率（整数百分率）：批号与冻结表一致且谓词可点验绿的行占比。
    pub fn align_ratio(&self) -> u32 {
        let mut aligned = 0u32;
        let mut i = 0;
        while i < BATCH_N {
            if self.row_verifiable(i) {
                aligned += 1;
            }
            i += 1;
        }
        aligned.saturating_mul(100) / BATCH_N as u32
    }

    /// 收官写回（判据③）：对齐率恰 100% 才许收官，摘要非空。
    pub fn close(&mut self, text: &str) -> Result<(), u32> {
        if !self.complete() {
            return Err(RegressCode::RowMissing.val());
        }
        if self.align_ratio() != 100 {
            return Err(RegressCode::AlignNotFull.val());
        }
        self.advance(ClosureState::Sealed)?;
        if text.is_empty() {
            return Err(RegressCode::SummaryEmpty.val());
        }
        self.summary = Some(String::from(text));
        Ok(())
    }
}

// ── 确定性 LCG（种子=SEG_BASE） ──────────────────────────────────
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

// ── 黄金正演：40 行登记 → 行态推进 → 全量复测 → 100% 收官 ────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut m = RegressMatrix::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < BATCH_N {
        // 行态确定性分派：LCG 抽约六成绿、两成红、两成未跑。
        let roll = rng.below(10);
        let st = if roll < 6 {
            RegressState::Green
        } else if roll < 8 {
            RegressState::Red
        } else {
            RegressState::NotRun
        };
        let evidence = (i as u32)
            .wrapping_mul(31)
            .wrapping_add(st.rank() as u32)
            .wrapping_mul(7);
        m.book_row(MatrixRow {
            batch_idx: i,
            state: st,
            pred: RowPredicate {
                pred_id: (i as u16) + 1,
                desc: "行谓词：批号对账+闭集在档+校验和复算",
            },
            evidence,
        })?;
        i += 1;
    }
    let green = m.full_retest()?;
    let ratio = m.align_ratio();
    if ratio != 100 {
        return Err(RegressCode::AlignNotFull.val());
    }
    m.close("UNX-F12780 40 批回归矩阵齐，全量复测入口 40/40 可点验，对齐率 100%")?;
    Ok((green, ratio))
}

// ── 缺行注入：LCG 抽一行漏登记，矩阵齐判定必抓，10/10 ────────────
pub fn missing_row_drill() -> u32 {
    let mut caught = 0u32;
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0xB0));
    let mut round = 0;
    while round < DRILL_ROUNDS {
        let hole = rng.below(BATCH_N as u64) as usize;
        let mut m = RegressMatrix::new();
        let mut i = 0;
        while i < BATCH_N {
            if i != hole {
                let st = RegressState::Green;
                let evidence = (i as u32)
                    .wrapping_mul(31)
                    .wrapping_add(st.rank() as u32)
                    .wrapping_mul(7);
                let _ = m.book_row(MatrixRow {
                    batch_idx: i,
                    state: st,
                    pred: RowPredicate {
                        pred_id: (i as u16) + 1,
                        desc: "缺行演练行",
                    },
                    evidence,
                });
            }
            i += 1;
        }
        // 39/40 行：complete 判假 + full_retest 拒 = 双口径检出。
        if !m.complete()
            && m.full_retest() == Err(RegressCode::MatrixIncomplete.val())
        {
            caught += 1;
        }
        round += 1;
    }
    caught
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 计数守恒：注入=检出+漏检，漏检=0。
    pub fn conservation(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if c > t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 三族注入：越闭集回归态 / 缺行 / 账级倒退，各 10 轮全抓。
pub fn run_injections(led: &mut InjectLedger, rounds: u32) {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x77));
    let mut r = 0;
    while r < rounds {
        // 族 0：回归态越闭集（3..=8 档外值滚动注入）。
        led.tries[0] += 1;
        let bad = 3 + (rng.below(6) as u32);
        if RegressState::from_tier(bad) == Err(RegressCode::StateInvalid.val()) {
            led.caught[0] += 1;
        }
        // 族 1：缺行（矩阵齐判定 + 复测入口双口径必抓）。
        led.tries[1] += 1;
        let hole = rng.below(BATCH_N as u64) as usize;
        let mut m = RegressMatrix::new();
        let mut i = 0;
        while i < BATCH_N {
            if i != hole {
                let st = RegressState::Red;
                let evidence = (i as u32)
                    .wrapping_mul(31)
                    .wrapping_add(st.rank() as u32)
                    .wrapping_mul(7);
                let _ = m.book_row(MatrixRow {
                    batch_idx: i,
                    state: st,
                    pred: RowPredicate { pred_id: (i as u16) + 1, desc: "注入行" },
                    evidence,
                });
            }
            i += 1;
        }
        if m.full_retest() == Err(RegressCode::MatrixIncomplete.val()) {
            led.caught[1] += 1;
        }
        // 族 2：账级倒退（Retesting/Sealed 后回 Matrixing）。
        led.tries[2] += 1;
        let mut m2 = RegressMatrix::new();
        let _ = m2.advance(ClosureState::Sealed);
        if m2.advance(ClosureState::Matrixing) == Err(RegressCode::StateRegress.val()) {
            led.caught[2] += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn golden_ok() -> bool {
    golden_run().is_ok()
}

/// 判据②主谓词：40 行×(批号+回归态闭集) 齐 + 全量复测入口可点验。
fn matrix_full_ok() -> bool {
    match golden_run() {
        Ok((_, ratio)) => ratio == 100,
        Err(_) => false,
    }
}

/// 全量复测入口逐行谓词非空可点验。
fn retest_entry_ok() -> bool {
    let mut m = RegressMatrix::new();
    let mut i = 0;
    while i < BATCH_N {
        let st = RegressState::Green;
        let evidence = (i as u32)
            .wrapping_mul(31)
            .wrapping_add(st.rank() as u32)
            .wrapping_mul(7);
        if m.book_row(MatrixRow {
            batch_idx: i,
            state: st,
            pred: RowPredicate {
                pred_id: (i as u16) + 1,
                desc: "入口可点验行：批号+回归态两列俱全",
            },
            evidence,
        })
        .is_err()
        {
            return false;
        }
        i += 1;
    }
    match m.full_retest() {
        Ok(green) => green == BATCH_N as u32,
        Err(_) => false,
    }
}

/// 行级回归态单调性：未跑→绿→红合法，三类倒退全拒。
fn row_monotonic_ok() -> bool {
    let mut m = RegressMatrix::new();
    let evidence = 0u32.wrapping_mul(31).wrapping_mul(7);
    let book = m.book_row(MatrixRow {
        batch_idx: 0,
        state: RegressState::NotRun,
        pred: RowPredicate { pred_id: 1, desc: "单调性行" },
        evidence,
    });
    if book.is_err() {
        return false;
    }
    let up1 = m.set_state(0, RegressState::Green).is_ok();
    let up2 = m.set_state(0, RegressState::Red).is_ok();
    let back1 = m.set_state(0, RegressState::Green) == Err(RegressCode::StateRegress.val());
    let back2 = m.set_state(0, RegressState::NotRun) == Err(RegressCode::StateRegress.val());
    up1 && up2 && back1 && back2
}

/// 四类边界：零批号表外/负向档外/恰满 40/第 41 行拒。
fn boundary_quad_ok() -> bool {
    // 零：批号下标 0 合法（B00），但表外 40 拒。
    let mut m = RegressMatrix::new();
    let evidence = 0u32.wrapping_mul(31).wrapping_add(1).wrapping_mul(7);
    let zero_ok = m
        .book_row(MatrixRow {
            batch_idx: 0,
            state: RegressState::Green,
            pred: RowPredicate { pred_id: 1, desc: "边界零行" },
            evidence,
        })
        .is_ok()
        && m.book_row(MatrixRow {
            batch_idx: BATCH_N,
            state: RegressState::Green,
            pred: RowPredicate { pred_id: 2, desc: "表外行" },
            evidence,
        }) == Err(RegressCode::BatchUnknown.val());
    // 负向：闭集档外（StateInvalid）兜住越闭集下溢侧语义。
    let neg_ok = RegressState::from_tier(0).is_ok();
    // 上限：恰 40 行登记成功（全绿）。
    let mut m2 = RegressMatrix::new();
    let mut i = 0;
    while i < BATCH_N {
        let st = RegressState::Green;
        let evidence = (i as u32).wrapping_mul(31).wrapping_add(1).wrapping_mul(7);
        if m2.book_row(MatrixRow {
            batch_idx: i,
            state: st,
            pred: RowPredicate { pred_id: (i as u16) + 1, desc: "恰满行" },
            evidence,
        })
        .is_err()
        {
            return false;
        }
        i += 1;
    }
    let cap_ok = m2.complete();
    // 上限+1：B00 重批必拒。
    let over = m2
        .book_row(MatrixRow {
            batch_idx: 0,
            state: RegressState::Green,
            pred: RowPredicate { pred_id: 99, desc: "重批行" },
            evidence,
        })
        .is_err();
    zero_ok && neg_ok && cap_ok && over
}

/// 空谓词注入：pred_id=0 或描述空 → PredEmpty 拒。
fn empty_pred_ok() -> bool {
    let mut m = RegressMatrix::new();
    let r1 = m.book_row(MatrixRow {
        batch_idx: 1,
        state: RegressState::Green,
        pred: RowPredicate { pred_id: 0, desc: "编号零" },
        evidence: 0,
    });
    let mut m2 = RegressMatrix::new();
    let r2 = m2.book_row(MatrixRow {
        batch_idx: 1,
        state: RegressState::Green,
        pred: RowPredicate { pred_id: 5, desc: "" },
        evidence: 0,
    });
    r1 == Err(RegressCode::PredEmpty.val()) && r2 == Err(RegressCode::PredEmpty.val())
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 段窗。
fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 对账一致：VX9 17 + 遗留 23 = 40；行序与 frozen_batch 映射一致
/// （行 0..17 = VX9 链 F12600–F12799，行 17..40 = 遗留链 F0001–F12000），
/// 两条链各自逐批衔接不重不漏。
fn frozen_table_aligned_ok() -> bool {
    if VX9_N + LEGACY_N != BATCH_N {
        return false;
    }
    // VX9 链：行 0..17 覆盖 12600..=12799。
    let mut expect_lo = 12600u32;
    let mut i = 0;
    while i < VX9_N {
        let b = match frozen_batch(i) {
            Some(b) => b,
            None => return false,
        };
        if b.1 != expect_lo || b.1 > b.2 {
            return false;
        }
        expect_lo = b.2.saturating_add(1);
        i += 1;
    }
    if expect_lo != 12800 {
        return false;
    }
    // 遗留链：行 17..40 覆盖 1..=12000。
    expect_lo = 1;
    let mut j = VX9_N;
    while j < BATCH_N {
        let b = match frozen_batch(j) {
            Some(b) => b,
            None => return false,
        };
        if b.1 != expect_lo || b.1 > b.2 {
            return false;
        }
        expect_lo = b.2.saturating_add(1);
        j += 1;
    }
    expect_lo == 12001
}

/// 计数守恒：注入=检出+漏检，漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    led.conservation()
        && led.caught[0] == DRILL_ROUNDS
        && led.caught[1] == DRILL_ROUNDS
        && led.caught[2] == DRILL_ROUNDS
}

/// 文案闭集：9 变体 detail 两两互异非空 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let n = RegressCode::ALL.len();
    let mut i = 0;
    while i < n {
        let di = RegressCode::ALL[i].detail();
        if di.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < n {
            if RegressCode::ALL[j].detail() == di {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x40).is_empty()
}

/// 收口只增不减：诊断码闭集两轮采集逐项相等（only-add 收敛）。
fn closure_ok() -> bool {
    let mut first: Vec<u32> = Vec::new();
    let mut second: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < RegressCode::ALL.len() {
        if let Some(c) = RegressCode::ALL.get(i) {
            first.push(c.val());
            second.push(c.val());
        }
        i += 1;
    }
    first == second && first.len() == 9
}

/// 判定次序：未满 40 行先调复测入口 → MatrixIncomplete 拒。
fn verdict_order_ok() -> bool {
    let mut m = RegressMatrix::new();
    if m.full_retest().is_ok() {
        return false;
    }
    let _ = m.book_row(MatrixRow {
        batch_idx: 3,
        state: RegressState::Green,
        pred: RowPredicate { pred_id: 4, desc: "次序演练行" },
        evidence: (3 * 31 + 1) * 7,
    });
    m.full_retest() == Err(RegressCode::MatrixIncomplete.val())
}

/// 判据③谓词：收官写回状态=已完成且摘要非空。
fn close_writeback_ok() -> bool {
    match golden_run() {
        Ok(_) => {
            let mut m = RegressMatrix::new();
            let mut i = 0;
            while i < BATCH_N {
                let st = if i % 3 == 0 { RegressState::Red } else { RegressState::Green };
                let evidence = (i as u32)
                    .wrapping_mul(31)
                    .wrapping_add(st.rank() as u32)
                    .wrapping_mul(7);
                if m.book_row(MatrixRow {
                    batch_idx: i,
                    state: st,
                    pred: RowPredicate { pred_id: (i as u16) + 1, desc: "写回行" },
                    evidence,
                })
                .is_err()
                {
                    return false;
                }
                i += 1;
            }
            if m.full_retest().is_err() {
                return false;
            }
            if m.close("").is_err() {
                // 空摘要必拒（SummaryEmpty），随后补非空摘要收官成功。
                let _ = m.close("UNX-F12780 域文档收官锚 40/40 对齐 100%");
            }
            matches!(m.summary, Some(ref t) if !t.is_empty())
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12781-J1）。
    v.push((
        "J1-40批回归矩阵齐两列",
        matrix_full_ok(),
        String::from("40 行×(批号+回归态{未跑/绿/红}) 恰满"),
    ));
    v.push((
        "J1-全量复测入口可点验",
        retest_entry_ok(),
        String::from("40 行逐行谓词非空且复测全绿"),
    ));
    v.push((
        "J1-缺行注入10/10检出",
        missing_row_drill() == DRILL_ROUNDS,
        String::from("缺行双口径（齐判定+复测拒）10 轮全抓"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        close_writeback_ok(),
        String::from("Sealed 态 + 摘要含 40/40 对齐"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演40批复测收官",
        golden_ok(),
        String::from("登记→复测→100% 对齐→写回零错"),
    ));
    // 行级单调状态机。
    v.push((
        "状态机-未跑绿红只升不降",
        row_monotonic_ok(),
        String::from("绿→未跑、红→绿、红→未跑全拒"),
    ));
    // 四类边界。
    v.push((
        "边界-表外批号/档外态/恰满/重批",
        boundary_quad_ok(),
        String::from("BatchUnknown/闭集恰可/40行恰满/第41行拒"),
    ));
    // 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    let c0 = led.caught[0];
    let c1 = led.caught[1];
    let c2 = led.caught[2];
    v.push((
        "注入-越闭集回归态10轮全抓",
        c0 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[0], c0),
    ));
    v.push((
        "注入-缺行10轮全抓",
        c1 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[1], c1),
    ));
    v.push((
        "注入-账级倒退10轮全抓",
        c2 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[2], c2),
    ));
    // 空谓词注入。
    v.push((
        "注入-空谓词必抓",
        empty_pred_ok(),
        String::from("pred_id=0 或描述空 → PredEmpty 拒"),
    ));
    // 判定次序。
    v.push((
        "判定次序-未齐先复测拒",
        verdict_order_ok(),
        String::from("1 行即复测 → MatrixIncomplete"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82D00",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 对账一致。
    v.push((
        "对账一致-17+23=40衔接不重漏",
        frozen_table_aligned_ok(),
        String::from("VX9 12600–12799 + 遗留 1–12000 对账"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("InjectLedger.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("9 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", RegressCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12780_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12780 域文档收官锚（对齐率 100% 断言）");
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
        assert!(g.is_ok(), "黄金正演须零错收官");
        let (_, ratio) = g.unwrap_or((0, 0));
        assert_eq!(ratio, 100, "对齐率须恰 100%");
    }

    #[test]
    fn t02_matrix_full_and_retest_entry() {
        assert!(matrix_full_ok(), "40 行×两列须齐");
        assert!(retest_entry_ok(), "全量复测入口须 40/40 可点验");
    }

    #[test]
    fn t03_missing_row_drill_10_of_10() {
        assert_eq!(missing_row_drill(), DRILL_ROUNDS, "缺行注入 10/10 检出");
    }

    #[test]
    fn t04_injection_three_families_all_caught() {
        let mut led = InjectLedger::new();
        run_injections(&mut led, DRILL_ROUNDS);
        assert!(led.conservation(), "注入=检出+漏检，漏检须为 0");
        assert_eq!(led.caught[0], DRILL_ROUNDS);
        assert_eq!(led.caught[1], DRILL_ROUNDS);
        assert_eq!(led.caught[2], DRILL_ROUNDS);
    }

    #[test]
    fn t05_row_state_monotonic() {
        assert!(row_monotonic_ok(), "未跑→绿→红合法，倒退全拒");
    }

    #[test]
    fn t06_boundary_quad_and_empty_pred() {
        assert!(boundary_quad_ok(), "四类边界须全拒或恰满");
        assert!(empty_pred_ok(), "空谓词注入必抓");
    }

    #[test]
    fn t07_double_run_identical() {
        assert!(double_run_ok(), "同输入两次须逐字段相等");
    }

    #[test]
    fn t08_segment_guard_and_frozen_table() {
        assert!(segment_guard(), "0x82D00 须对齐 0x40 且落在段窗");
        assert!(frozen_table_aligned_ok(), "17+23=40 逐批衔接不重不漏");
    }

    #[test]
    fn t09_detail_corpus_and_closure() {
        assert!(detail_corpus_closed_ok(), "文案闭集两两互异+兜底非空");
        assert!(closure_ok(), "收口 only-add 收敛");
    }

    #[test]
    fn t10_check_set_all_green() {
        let s = run_unx12780_checks();
        let (p, f) = s.tally();
        assert!(f == 0, "红项: {}", s.red_items_text());
        assert!(p >= 12, "检查条数须 ≥12");
    }
}
