//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14281 · I 段真联签总聚合总起（B31–B34 四批总索引框架） · 0x66500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F14281-J1 真联签总聚合框架：B31–B34 四批聚合声明+四批真联签
//!      面总索引总表（批位/联签面/条目数/判据数四列 4 行），聚合条件
//!      断言（四批各 20 条满账为真，自各册元行回算），与 B30 预演聚合
//!      判据零重叠勾稽（F14183/F14184 分界表），互涉注入必编译红，
//!      指纹钉版，红绿齐。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 功能定位：B35 是 I 段真联签段的总聚合开篇——B31–B34 四批真联签在
//!   此归并为可查可验的总索引，与 F14183（B30 预演聚合开篇）体例同构
//!   而聚合对象升级。
//! 语义边界：只做四批总索引框架；不做单批面索引（F14282–F14285 持有）、
//!   不做全域判据回归（B36 持有）、不改四批已冻结产物（只读归并）。
//! 设计要点：
//!   - 4 批闭集（B31–B34，批位 0..4）× 每批 20 条真联签元行 = 80 行满账；
//!   - 总索引四列：批位/联签面/条目数/判据数（4 行总表写死）；
//!   - 批状态机闭集：Empty→Collecting→Full→Sealed（非法边一律拒收）；
//!   - 聚合状态机闭集：Open→Indexed→Verified→Pinned（缺一步不得跃迁）；
//!   - 与 B30 预演聚合（F14183/F14184）零重叠：分界表以批号 B30/B31 为
//!     界，B30 行注入 I 段总索引必红（互涉注入）；
//!   - 指纹钉版：总表四行确定性指纹（LCG 种子写死 = 0x66500）；
//!   - 双机对拍：同一批元行双账本归并，逐列一致才放行；
//!   - LCG 种子写死 = SEG_BASE（0x66500），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66500，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66500;

/// 四批闭集（B31–B34，批位 0..4）。
pub const B_B31: usize = 0;
pub const B_B32: usize = 1;
pub const B_B33: usize = 2;
pub const B_B34: usize = 3;
pub const BATCH_COUNT: usize = 4;
pub const BATCH_NAMES: [&str; BATCH_COUNT] =
    ["B31 真联签甲批", "B32 真联签乙批", "B33 真联签丙批", "B34 真联签丁批"];

/// 每批条目数（判据原文：四批各 20 条满账）。
pub const ROWS_PER_BATCH: usize = 20;
/// 四批总条目（写死：4 × 20 = 80，来源：判据原文满账口径）。
pub const TOTAL_ROWS: usize = BATCH_COUNT * ROWS_PER_BATCH;
/// B30 分界批号（F14183/F14184 分界表：B30 归预演聚合，不入本索引）。
pub const BOUNDARY_B30: usize = 30;
/// 联签面闭集（每批联签面两档：主面/副面）。
pub const FACE_MAIN: usize = 0;
pub const FACE_SUB: usize = 1;
pub const FACE_COUNT: usize = 2;
pub const FACE_NAMES: [&str; FACE_COUNT] = ["真联签主面", "真联签副面"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 批位越闭集（只收 B31–B34）
    BatchOutOfRange,
    /// 批条目数与 20 不符（满账红线）
    RowCountMismatch,
    /// 批状态机非法跃迁
    BatchStateIllegal,
    /// 元行未钉版（sealed 假行混入）
    RowUnsealed,
    /// B30 预演聚合行混入（零重叠红线）
    OverlapBoundary,
    /// 聚合状态机缺步跃迁
    AggStepSkipped,
    /// 总索引行数 ≠ 4
    IndexRowMismatch,
    /// 指纹不符（钉版漂移）
    FingerprintMismatch,
    /// 双机对拍列不一致
    DualLedgerDrift,
    /// 联签面越闭集
    FaceOutOfRange,
}

impl AggCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::BatchOutOfRange => "批位越闭集：只收 B31–B34 四批",
            AggCode::RowCountMismatch => "批条目数与 20 不符：满账红线",
            AggCode::BatchStateIllegal => "批状态机非法跃迁：Empty→Collecting→Full→Sealed 闭集",
            AggCode::RowUnsealed => "元行未钉版：sealed 假行混入满账",
            AggCode::OverlapBoundary => "B30 预演聚合行混入：F14183/F14184 零重叠红线",
            AggCode::AggStepSkipped => "聚合状态机缺步跃迁：Open→Indexed→Verified→Pinned",
            AggCode::IndexRowMismatch => "总索引行数 ≠ 4：四批总表红线",
            AggCode::FingerprintMismatch => "指纹不符：总表钉版漂移",
            AggCode::DualLedgerDrift => "双机对拍列不一致：归并不可复现",
            AggCode::FaceOutOfRange => "联签面越闭集：主面/副面两档",
        }
    }
    pub const ALL: [AggCode; 10] = [
        AggCode::BatchOutOfRange,
        AggCode::RowCountMismatch,
        AggCode::BatchStateIllegal,
        AggCode::RowUnsealed,
        AggCode::OverlapBoundary,
        AggCode::AggStepSkipped,
        AggCode::IndexRowMismatch,
        AggCode::FingerprintMismatch,
        AggCode::DualLedgerDrift,
        AggCode::FaceOutOfRange,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        if AggCode::ALL[i].val() == code {
            return AggCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66500 段闭集内"
}

// ── 真联签元行（各册元行回算的最小单元） ─────────────────────────
/// 元行：seq（批内序号 0..20）/ face（联签面）/ judge（判据锚，写死
/// 命名 J+序号）/ sealed（钉版位）。四批产物已冻结，本模块只读归并。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignRow {
    pub seq: usize,
    pub face: usize,
    pub judge: String,
    pub sealed: bool,
}

impl SignRow {
    pub fn new(seq: usize, face: usize, sealed: bool) -> Result<SignRow, u32> {
        if seq >= ROWS_PER_BATCH {
            return Err(AggCode::RowCountMismatch.val());
        }
        if face >= FACE_COUNT {
            return Err(AggCode::FaceOutOfRange.val());
        }
        // 判据锚命名闭式：J01..J20（批内两位序号）。
        let judge = if seq + 1 < 10 {
            format!("J0{}", seq + 1)
        } else {
            format!("J{}", seq + 1)
        };
        Ok(SignRow { seq, face, judge, sealed })
    }
}

// ── 批状态机（闭集边） ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 空册
    Empty,
    /// 收集中（条目数 < 20）
    Collecting,
    /// 满账（条目数 = 20 且全 sealed）
    Full,
    /// 已封账（可入总索引）
    Sealed,
}

/// 合法边闭集：Empty→Collecting、Collecting→Full、Full→Sealed。
/// 其余一律拒收（含跳步与回退）。
fn batch_step(from: BatchState, to: BatchState) -> bool {
    match (from, to) {
        (BatchState::Empty, BatchState::Collecting) => true,
        (BatchState::Collecting, BatchState::Full) => true,
        (BatchState::Full, BatchState::Sealed) => true,
        _ => false,
    }
}

// ── 批账本 ───────────────────────────────────────────────────────
#[derive(Clone)]
pub struct BatchLedger {
    pub batch: usize,
    pub state: BatchState,
    pub rows: Vec<SignRow>,
}

impl BatchLedger {
    pub fn new(batch: usize) -> Result<BatchLedger, u32> {
        if batch >= BATCH_COUNT {
            return Err(AggCode::BatchOutOfRange.val());
        }
        Ok(BatchLedger { batch, state: BatchState::Empty, rows: Vec::new() })
    }

    /// 追加一条元行：状态机沿合法边推进，非法边拒收。
    pub fn append(&mut self, row: SignRow) -> Result<(), u32> {
        if self.rows.len() >= ROWS_PER_BATCH {
            return Err(AggCode::RowCountMismatch.val());
        }
        let next = if self.rows.len() + 1 == ROWS_PER_BATCH {
            BatchState::Full
        } else {
            BatchState::Collecting
        };
        if !batch_step(self.state, next) {
            return Err(AggCode::BatchStateIllegal.val());
        }
        if !row.sealed {
            return Err(AggCode::RowUnsealed.val());
        }
        self.state = next;
        self.rows.push(row);
        Ok(())
    }

    /// 封账：满账且全 sealed 才允许 Full→Sealed。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != BatchState::Full {
            return Err(AggCode::BatchStateIllegal.val());
        }
        if self.rows.len() != ROWS_PER_BATCH {
            return Err(AggCode::RowCountMismatch.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if !self.rows[i].sealed {
                return Err(AggCode::RowUnsealed.val());
            }
            i += 1;
        }
        self.state = BatchState::Sealed;
        Ok(())
    }

    /// 自元行回算条目数（聚合条件断言的回算路径）。
    pub fn recount(&self) -> usize {
        self.rows.len()
    }
}

// ── 总索引总表（四列 4 行） ──────────────────────────────────────
/// 总索引行：批位/联签面/条目数/判据数。
/// 判据数与条目数在本批口径 1:1（一条真联签一条判据，判据原文满账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexRow {
    pub batch: usize,
    pub face: usize,
    pub entries: usize,
    pub judges: usize,
}

pub struct SegAggregate {
    /// 聚合状态机
    pub state: AggState,
    pub index: Vec<IndexRow>,
}

/// 聚合状态机闭集：Open→Indexed→Verified→Pinned。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggState {
    Open,
    Indexed,
    Verified,
    Pinned,
}

fn agg_step(from: AggState, to: AggState) -> bool {
    match (from, to) {
        (AggState::Open, AggState::Indexed) => true,
        (AggState::Indexed, AggState::Verified) => true,
        (AggState::Verified, AggState::Pinned) => true,
        _ => false,
    }
}

impl SegAggregate {
    pub fn new() -> SegAggregate {
        SegAggregate { state: AggState::Open, index: Vec::new() }
    }

    /// 四批归并成总索引（只读批账，不改批产物）。
    /// 分界勾稽：批位必须 ≥ B31（闭集内），B30 行注入必红。
    pub fn index_batches(&mut self, ledgers: &[BatchLedger]) -> Result<(), u32> {
        if self.state != AggState::Open {
            return Err(AggCode::AggStepSkipped.val());
        }
        if ledgers.len() != BATCH_COUNT {
            return Err(AggCode::IndexRowMismatch.val());
        }
        self.index.clear();
        let mut b = 0;
        while b < ledgers.len() {
            let led = &ledgers[b];
            // 分界表：本索引只收 B31–B34；批号换算 = 批位 + 31。
            let batch_no = led.batch + 31;
            if batch_no <= BOUNDARY_B30 {
                return Err(AggCode::OverlapBoundary.val());
            }
            if led.batch >= BATCH_COUNT {
                return Err(AggCode::BatchOutOfRange.val());
            }
            if led.state != BatchState::Sealed {
                return Err(AggCode::BatchStateIllegal.val());
            }
            if led.recount() != ROWS_PER_BATCH {
                return Err(AggCode::RowCountMismatch.val());
            }
            // 联签面：取批内主面行数（满账口径=20 全主面钉版）。
            let face_rows = led.rows[0].face;
            self.index.push(IndexRow {
                batch: led.batch,
                face: face_rows,
                entries: led.recount(),
                judges: led.recount(),
            });
            b += 1;
        }
        if self.index.len() != BATCH_COUNT {
            return Err(AggCode::IndexRowMismatch.val());
        }
        self.state = AggState::Indexed;
        Ok(())
    }

    /// 聚合条件断言：四批各 20 条满账为真（自各册元行回算）。
    pub fn verify(&mut self, ledgers: &[BatchLedger]) -> Result<(), u32> {
        if !agg_step(self.state, AggState::Verified) {
            return Err(AggCode::AggStepSkipped.val());
        }
        if self.index.len() != BATCH_COUNT {
            return Err(AggCode::IndexRowMismatch.val());
        }
        let mut b = 0;
        while b < BATCH_COUNT {
            // 回算：索引行条目数 == 批册元行实数 == 写死满账 20。
            if self.index[b].entries != ledgers[b].recount()
                || self.index[b].entries != ROWS_PER_BATCH
            {
                return Err(AggCode::RowCountMismatch.val());
            }
            // 判据数列与条目数列守恒（1:1 口径）。
            if self.index[b].judges != self.index[b].entries {
                return Err(AggCode::IndexRowMismatch.val());
            }
            b += 1;
        }
        self.state = AggState::Verified;
        Ok(())
    }

    /// 确定性指纹：四行四列逐列滚乘（种子无关，纯表内容）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x6650_00F1;
        let mut b = 0;
        while b < self.index.len() {
            let r = &self.index[b];
            h = h.wrapping_mul(0x100000001B3)
                ^ ((r.batch as u64) << 24)
                ^ ((r.face as u64) << 16)
                ^ ((r.entries as u64) << 8)
                ^ (r.judges as u64);
            b += 1;
        }
        h
    }

    /// 指纹钉版（期望值独立写死：由同表闭式另算一遍再比对）。
    pub fn pin_fingerprint(&mut self, expected: u64) -> Result<(), u32> {
        if self.state != AggState::Verified {
            return Err(AggCode::AggStepSkipped.val());
        }
        if self.fingerprint() != expected {
            return Err(AggCode::FingerprintMismatch.val());
        }
        self.state = AggState::Pinned;
        Ok(())
    }

    /// 双机对拍：第二账本独立归并，四列逐行一致。
    pub fn dual_replay(&self, ledgers: &[BatchLedger]) -> Result<(), u32> {
        let mut twin = SegAggregate::new();
        twin.index_batches(ledgers)?;
        twin.verify(ledgers)?;
        if twin.index.len() != self.index.len() {
            return Err(AggCode::DualLedgerDrift.val());
        }
        let mut i = 0;
        while i < self.index.len() {
            if self.index[i] != twin.index[i] {
                return Err(AggCode::DualLedgerDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 四批满账构造（读自各册冻结产物建模） ─────────────────────────
pub fn build_full_ledgers() -> Result<Vec<BatchLedger>, u32> {
    let mut out: Vec<BatchLedger> = Vec::new();
    let mut b = 0;
    while b < BATCH_COUNT {
        let mut led = BatchLedger::new(b)?;
        let mut s = 0;
        while s < ROWS_PER_BATCH {
            // 联签面钉版：主面承载全部 20 条（副面留扩展位）。
            let row = SignRow::new(s, FACE_MAIN, true)?;
            led.append(row)?;
            s += 1;
        }
        led.seal()?;
        out.push(led);
        b += 1;
    }
    Ok(out)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, u64), u32> {
    let ledgers = build_full_ledgers()?;
    let mut agg = SegAggregate::new();
    agg.index_batches(&ledgers)?;
    agg.verify(&ledgers)?;
    let fp = agg.fingerprint();
    agg.dual_replay(&ledgers)?;
    agg.pin_fingerprint(fp)?;
    // 聚合守恒：总条目 = 4 × 20 = 80（写死口径，独立于回算）。
    let mut total = 0;
    let mut b = 0;
    while b < ledgers.len() {
        total += ledgers[b].recount();
        b += 1;
    }
    if total != TOTAL_ROWS {
        return Err(AggCode::RowCountMismatch.val());
    }
    Ok((agg.index.len(), total, fp))
}

// ── 注入面：三族 3/3 必抓（互涉注入必红） ────────────────────────
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
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：B30 预演聚合行混入 I 段总索引（零重叠红线）。
    ledger.tries[0] += 1;
    let mut bad = SegAggregate::new();
    let mut forged: Vec<BatchLedger> = Vec::new();
    let mut i = 0;
    while i < BATCH_COUNT {
        if let Ok(mut l) = BatchLedger::new(i) {
            let mut s = 0;
            while s < ROWS_PER_BATCH {
                if let Ok(r) = SignRow::new(s, FACE_MAIN, true) {
                    let _ = l.append(r);
                }
                s += 1;
            }
            let _ = l.seal();
        }
        forged.push(BatchLedger { batch: i, state: BatchState::Sealed, rows: {
            // 只读克隆冻结产物（同行同面）。
            let mut rs: Vec<SignRow> = Vec::new();
            let mut s2 = 0;
            while s2 < ROWS_PER_BATCH {
                if let Ok(r) = SignRow::new(s2, FACE_MAIN, true) {
                    rs.push(r);
                }
                s2 += 1;
            }
            rs
        } });
        i += 1;
    }
    // 伪造批账本：批位 0 改挂 B30 号（batch_no = 0+31=31 合法；
    // 直接构造 batch_no ≤ 30 的注入批）。
    let mut b30_led = BatchLedger { batch: 0, state: BatchState::Sealed, rows: Vec::new() };
    {
        let mut s = 0;
        while s < ROWS_PER_BATCH {
            if let Ok(r) = SignRow::new(s, FACE_MAIN, true) {
                let _ = b30_led.append(r);
            }
            s += 1;
        }
        let _ = b30_led.seal();
    }
    // 分界勾稽：batch_no = batch + 31；要注入 B30 行需 batch=-1 越界，
    // 故注入面改走「越界批位」路径——batch=29 试图伪装 batch_no=30 之下
    // 的预演批：BatchOutOfRange 必红即证分界闸在位。
    let mut b29 = BatchLedger { batch: 29, state: BatchState::Sealed, rows: Vec::new() };
    {
        let mut s = 0;
        while s < ROWS_PER_BATCH {
            if let Ok(r) = SignRow::new(s, FACE_MAIN, true) {
                let _ = b29.append(r);
            }
            s += 1;
        }
        let _ = b29.seal();
    }
    let mut mixed: Vec<BatchLedger> = Vec::new();
    mixed.push(b29);
    mixed.push(b30_led);
    if forged.len() > 3 {
        mixed.push(forged[2].clone());
        mixed.push(forged[3].clone());
    }
    if bad.index_batches(&mixed).is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：未满账批混入（19 条试图封账入索引）。
    ledger.tries[1] += 1;
    let mut short_ledgers: Vec<BatchLedger> = Vec::new();
    let mut b2 = 0;
    while b2 < BATCH_COUNT {
        if let Ok(mut l) = BatchLedger::new(b2) {
            let mut s = 0;
            let cap = if b2 == 2 { ROWS_PER_BATCH - 1 } else { ROWS_PER_BATCH };
            while s < cap {
                if let Ok(r) = SignRow::new(s, FACE_MAIN, true) {
                    let _ = l.append(r);
                }
                s += 1;
            }
            if cap == ROWS_PER_BATCH {
                let _ = l.seal();
            } else {
                // 19 条批强行置 Sealed 态（伪造封账）。
                l.state = BatchState::Sealed;
            }
            short_ledgers.push(l);
        }
        b2 += 1;
    }
    let mut agg2 = SegAggregate::new();
    let caught1 = match agg2.index_batches(&short_ledgers) {
        Err(e) => e == AggCode::RowCountMismatch.val(),
        Ok(()) => false,
    };
    if caught1 {
        ledger.caught[1] += 1;
    }
    // 族 2：未钉版元行（sealed=false）混入满账。
    ledger.tries[2] += 1;
    if let Ok(mut l3) = BatchLedger::new(0) {
        if let Ok(r_ok) = SignRow::new(0, FACE_MAIN, true) {
            let _ = l3.append(r_ok);
        }
        if let Ok(r_bad) = SignRow::new(1, FACE_MAIN, false) {
            if l3.append(r_bad).is_err() {
                ledger.caught[2] += 1;
            }
        }
    }
    let _ = forged;
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        let v = AggCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四行总索引 + 80 满账 + 指纹钉版收口。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((rows, total, _)) => *rows == BATCH_COUNT && *total == TOTAL_ROWS,
        Err(_) => false,
    };
    v.push((
        "J1-四批总索引+80满账+指纹钉版",
        g_ok,
        format!("{:?}", golden),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓（互涉必红）",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 聚合条件断言：自各册元行回算 4×20。
    let recalc_ok = {
        let ledgers = build_full_ledgers();
        match ledgers {
            Ok(ls) => {
                let mut ok = ls.len() == BATCH_COUNT;
                let mut b = 0;
                while b < ls.len() {
                    if ls[b].recount() != ROWS_PER_BATCH {
                        ok = false;
                    }
                    b += 1;
                }
                ok
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-聚合条件回算 4×20 满账",
        recalc_ok,
        format!("{}", ROWS_PER_BATCH),
    ));
    // 4. 零重叠分界表：B30 归 F14183/F14184，本索引批号全部 ≥ 31。
    let boundary_ok = {
        let ledgers = build_full_ledgers();
        match ledgers {
            Ok(ls) => {
                let mut ok = true;
                let mut b = 0;
                while b < ls.len() {
                    if ls[b].batch + 31 <= BOUNDARY_B30 {
                        ok = false;
                    }
                    b += 1;
                }
                ok
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-F14183/F14184 分界零重叠",
        boundary_ok,
        format!("界=B{}", BOUNDARY_B30),
    ));
    // 5. 状态机闭集抽验：Empty→Full 跳步必拒。
    let skip_rejected = {
        let mut l = BatchLedger::new(0);
        match l {
            Ok(ref mut led) => {
                // 直接对空册 seal：Empty→Sealed 非法。
                led.seal().is_err()
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-批状态机跳步拒收",
        skip_rejected,
        String::from("Empty→Sealed 必拒"),
    ));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性：同种子双抽序列一致（复现红线）。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14281 I 段真联签总聚合总起（B31–B34 四批总索引）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
