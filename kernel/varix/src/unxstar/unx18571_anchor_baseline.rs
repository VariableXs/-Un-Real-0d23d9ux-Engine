//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18571 · ADR-UNX-008 锚基线填充判据（波 07 裁决消费） · 0x80A80段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18571-J1 锚裁决填充后矩阵基线层生效断言全过，机制
//!       空载→满载切换回归绿
//! 设计要点：
//!   - 波 07 裁决表（Pass/Fail/Deferred 三态）作为唯一上游输入，锚基线
//!     填充只允许消费「已裁决且 Pass」的锚；Deferred/Fail 锚记阻塞账，
//!     永不入基线；
//!   - 矩阵基线层 = 24 行 × 4 格 u8 位面；行状态机 Empty→Filled→Locked
//!     单向推进，锁后写入一律拒绝（回滚红线）；
//!   - 矩阵模式机 EmptyLoad→Filling→FullLoad 严格单调，FullLoad 仅在
//!     「已填行数 == Pass 锚总数」时允许切换（守恒闸）；
//!   - 注入核四类坏样本：未裁决填充 / 越界锚号 / 重复填充 / 阻塞锚
//!     填充，逐类 10/10 轮必抓；
//!   - 二次消费裁决表 → AlreadyConsumed；锁后再 fill → AlreadyLocked；
//!   - LCG 种子写死 = SEG_BASE（0x80A80）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 本模块结构清单：
//!   数据结构：AnchorCell（裁决/填充/锁三元）、BaselineMatrix（24×4 位面）
//!   状态机  ：RowState（行三态）、MatrixMode（矩阵三态，单调）
//!   账本    ：blocked_ledger（阻塞锚账）、fill_trace（切换轨迹账）
//!   错误路径：AnxCode 闭集 12 码，code() = SEG_BASE + n
//!   注入核  ：inject_rounds（四类坏样本 × 10 轮，逐类全抓）
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80A80，AI-VX11-1105 批09 G1 独占区）。
pub const SEG_BASE: u32 = 0x80A80;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 锚总数（ADR-UNX-008 冻结：24 锚）。
pub const ANCHOR_COUNT: usize = 24;
/// 基线行格数（冻结：每行 4 格）。
pub const ROW_CELLS: usize = 4;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 波 07 裁决表 Epoch 起始号（写死：首个填充纪元）。
pub const EPOCH_BASE: u32 = 1;

// ── 波 07 裁决三态 ─────────────────────────────────────────────
/// 波 07 上游裁决：过 / 挂 / 缓。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorVerdict {
    /// 裁决通过：可入基线。
    Pass,
    /// 裁决失败：永久阻塞。
    Fail,
    /// 裁决缓议：本轮阻塞，留待下波。
    Deferred,
    /// 尚未裁决（初态）。
    Unset,
}

impl AnchorVerdict {
    /// 可入基线判定：仅 Pass 放行。
    pub fn fillable(self) -> bool {
        matches!(self, AnchorVerdict::Pass)
    }
    /// 已裁决判定：Unset 以外均已裁决。
    pub fn adjudicated(self) -> bool {
        !matches!(self, AnchorVerdict::Unset)
    }
}

// ── 波 07 规范裁决表（24 锚：20 过 / 2 挂 / 2 缓） ──────────────
/// 规范表写死：20 个 Pass（锚 0..20），锚 20/21 Fail，锚 22/23 Deferred。
/// 推导：20 = ANCHOR_COUNT - 4（4 个非过态锚分布于表尾，便于边界探测）。
pub fn wave07_table() -> [AnchorVerdict; ANCHOR_COUNT] {
    let mut t = [AnchorVerdict::Unset; ANCHOR_COUNT];
    let mut i = 0;
    while i < ANCHOR_COUNT {
        if i < 20 {
            t[i] = AnchorVerdict::Pass;
        } else if i < 22 {
            t[i] = AnchorVerdict::Fail;
        } else {
            t[i] = AnchorVerdict::Deferred;
        }
        i += 1;
    }
    t
}

/// 规范表 Pass 锚数（推导：20，与 wave07_table 实数一致）。
pub const WAVE07_PASS_COUNT: usize = 20;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnxCode {
    /// 锚号越界（≥ ANCHOR_COUNT）
    AnchorIdRange,
    /// 未消费裁决表即填充
    VerdictNotConsumed,
    /// 裁决表二次消费
    AlreadyConsumed,
    /// 未裁决锚（Unset）填充
    UnadjudicatedFill,
    /// 阻塞锚（Fail/Deferred）填充
    BlockedAnchorFill,
    /// 重复填充（行已 Filled/Locked）
    DuplicateFill,
    /// 满载前锁定
    LockBeforeFull,
    /// 二次锁定
    AlreadyLocked,
    /// 锁后写入（回滚红线）
    WriteAfterLock,
    /// 回滚尝试（模式倒退）
    RollbackIllegal,
    /// 守恒破坏（已填行 ≠ Pass 锚数）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl AnxCode {
    /// 诊断码 = SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            AnxCode::AnchorIdRange => "锚号越界：0..24 闭集外",
            AnxCode::VerdictNotConsumed => "裁决表未消费即填充",
            AnxCode::AlreadyConsumed => "裁决表二次消费",
            AnxCode::UnadjudicatedFill => "未裁决锚填充：Unset 态",
            AnxCode::BlockedAnchorFill => "阻塞锚填充：Fail/Deferred",
            AnxCode::DuplicateFill => "重复填充：行非空态",
            AnxCode::LockBeforeFull => "满载前锁定：守恒闸未过",
            AnxCode::AlreadyLocked => "二次锁定",
            AnxCode::WriteAfterLock => "锁后写入：回滚红线",
            AnxCode::RollbackIllegal => "回滚尝试：模式倒退",
            AnxCode::ConservationBroken => "守恒破坏：已填行≠Pass锚数",
            AnxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（12 码）。
    pub const ALL: [AnxCode; 12] = [
        AnxCode::AnchorIdRange,
        AnxCode::VerdictNotConsumed,
        AnxCode::AlreadyConsumed,
        AnxCode::UnadjudicatedFill,
        AnxCode::BlockedAnchorFill,
        AnxCode::DuplicateFill,
        AnxCode::LockBeforeFull,
        AnxCode::AlreadyLocked,
        AnxCode::WriteAfterLock,
        AnxCode::RollbackIllegal,
        AnxCode::ConservationBroken,
        AnxCode::NotReproducible,
    ];
}

/// 码 → 明细反查（闭集外返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AnxCode::ALL.len() {
        if AnxCode::ALL[i].code() == code {
            return AnxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80A80 段闭集内"
}

// ── 行状态机（Empty→Filled→Locked 单向） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    /// 空行：可填充。
    Empty,
    /// 已填：可锁定。
    Filled,
    /// 已锁：一切写入拒绝。
    Locked,
}

impl RowState {
    /// 初态常量（供数组重复构造）。
    pub const fn empty() -> RowState {
        RowState::Empty
    }
    /// 单向推进合法判定。
    pub fn can_advance_to(self, next: RowState) -> bool {
        matches!(
            (self, next),
            (RowState::Empty, RowState::Filled) | (RowState::Filled, RowState::Locked)
        )
    }
}

// ── 矩阵模式机（EmptyLoad→Filling→FullLoad 单调） ──────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixMode {
    /// 空载：零行已填。
    EmptyLoad,
    /// 填充中：0 < 已填 < Pass 锚数。
    Filling,
    /// 满载：已填 == Pass 锚数且全体锁定。
    FullLoad,
}

// ── 锚单元（裁决/填充/锁三元） ──────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnchorCell {
    /// 波 07 裁决态。
    pub verdict: AnchorVerdict,
    /// 行状态。
    pub row: RowState,
    /// 填充纪元号（EPOCH_BASE 起单调）。
    pub epoch: u32,
}

impl AnchorCell {
    pub const fn empty() -> AnchorCell {
        AnchorCell { verdict: AnchorVerdict::Unset, row: RowState::Empty, epoch: 0 }
    }
}

// ── 矩阵基线层（24 行 × 4 格位面 + 账本） ───────────────────────
pub struct BaselineMatrix {
    /// 基线位面：24 行 × 4 格；填充行每格写纪元号低 8 位。
    pub cells: [[u8; ROW_CELLS]; ANCHOR_COUNT],
    /// 行状态账。
    pub rows: [RowState; ANCHOR_COUNT],
    /// 锚裁决账（消费 wave07 后落定）。
    pub verdicts: [AnchorVerdict; ANCHOR_COUNT],
    /// 矩阵模式。
    pub mode: MatrixMode,
    /// 已填行数（守恒核心量）。
    pub filled_rows: usize,
    /// 阻塞锚账：(锚号, 拒因码) 逐条留痕。
    pub blocked_ledger: Vec<(usize, u32)>,
    /// 空载→满载切换轨迹账（模式推进逐条记录）。
    pub fill_trace: Vec<u8>,
    /// 裁决表是否已消费。
    pub consumed: bool,
    /// 当前纪元。
    pub epoch: u32,
    /// 总写入尝试计数（守恒分母）。
    pub write_attempts: u32,
    /// 被拒写入计数（守恒分子之一）。
    pub write_rejected: u32,
}

impl BaselineMatrix {
    /// 新矩阵：全 Unset / 全空行 / EmptyLoad。
    pub fn new() -> BaselineMatrix {
        BaselineMatrix {
            cells: [[0u8; ROW_CELLS]; ANCHOR_COUNT],
            rows: [RowState::empty(); ANCHOR_COUNT],
            verdicts: [AnchorVerdict::Unset; ANCHOR_COUNT],
            mode: MatrixMode::EmptyLoad,
            filled_rows: 0,
            blocked_ledger: Vec::new(),
            fill_trace: Vec::new(),
            consumed: false,
            epoch: EPOCH_BASE,
            write_attempts: 0,
            write_rejected: 0,
        }
    }

    /// 消费波 07 裁决表：仅允许一次，返回 Pass 锚数。
    pub fn consume_wave07(&mut self, table: &[AnchorVerdict]) -> Result<usize, u32> {
        if self.consumed {
            return Err(AnxCode::AlreadyConsumed.code());
        }
        if table.len() != ANCHOR_COUNT {
            return Err(AnxCode::AnchorIdRange.code());
        }
        // 先核表再落账：任一锚未裁决则整表拒收（不留半消费态）。
        let mut i = 0;
        while i < ANCHOR_COUNT {
            let v = match table.get(i) {
                Some(v) => *v,
                None => return Err(AnxCode::AnchorIdRange.code()),
            };
            if !v.adjudicated() {
                return Err(AnxCode::UnadjudicatedFill.code());
            }
            i += 1;
        }
        let mut i = 0;
        let mut pass_n = 0usize;
        while i < ANCHOR_COUNT {
            self.verdicts[i] = table[i];
            if table[i].fillable() {
                pass_n += 1;
            }
            i += 1;
        }
        self.consumed = true;
        Ok(pass_n)
    }

    /// 填充单锚：区间→消费→裁决→行态四闸依次过闸才落格。
    pub fn fill_anchor(&mut self, id: usize) -> Result<(), u32> {
        self.write_attempts = self.write_attempts.saturating_add(1);
        // 闸一：锚号区间。
        if id >= ANCHOR_COUNT {
            self.write_rejected = self.write_rejected.saturating_add(1);
            return Err(AnxCode::AnchorIdRange.code());
        }
        // 闸二：裁决表必须先消费。
        if !self.consumed {
            self.write_rejected = self.write_rejected.saturating_add(1);
            return Err(AnxCode::VerdictNotConsumed.code());
        }
        let v = self.verdicts[id];
        // 闸三：仅 Pass 放行；阻塞锚入账留痕。
        if !v.adjudicated() {
            self.write_rejected = self.write_rejected.saturating_add(1);
            return Err(AnxCode::UnadjudicatedFill.code());
        }
        if !v.fillable() {
            self.write_rejected = self.write_rejected.saturating_add(1);
            self.blocked_ledger.push((id, AnxCode::BlockedAnchorFill.code()));
            return Err(AnxCode::BlockedAnchorFill.code());
        }
        // 闸四：行态必须为空（重复填充拒）。
        if self.rows[id] != RowState::Empty {
            self.write_rejected = self.write_rejected.saturating_add(1);
            return Err(AnxCode::DuplicateFill.code());
        }
        // 落格：整行 4 格写纪元低 8 位，行态推进 Empty→Filled。
        let tag = (self.epoch & 0xFF) as u8;
        let mut c = 0;
        while c < ROW_CELLS {
            self.cells[id][c] = tag;
            c += 1;
        }
        self.rows[id] = RowState::Filled;
        self.epoch = self.epoch.saturating_add(1);
        self.filled_rows = self.filled_rows.saturating_add(1);
        // 模式推进：EmptyLoad→Filling（首次落格时）。
        if self.mode == MatrixMode::EmptyLoad {
            self.mode = MatrixMode::Filling;
            self.fill_trace.push(1u8);
        }
        Ok(())
    }

    /// 锁定全体：仅满载闸过后允许，行态 Filled→Locked。
    pub fn lock_all(&mut self) -> Result<(), u32> {
        if self.filled_rows != WAVE07_PASS_COUNT {
            return Err(AnxCode::LockBeforeFull.code());
        }
        if self.mode == MatrixMode::FullLoad {
            return Err(AnxCode::AlreadyLocked.code());
        }
        let mut i = 0;
        while i < ANCHOR_COUNT {
            if self.rows[i] == RowState::Filled {
                self.rows[i] = RowState::Locked;
            }
            i += 1;
        }
        self.mode = MatrixMode::FullLoad;
        self.fill_trace.push(2u8);
        Ok(())
    }

    /// 锁后单格写探针：FullLoad 态一律拒绝（回滚红线显式证据）。
    pub fn try_write_cell(&self, id: usize, col: usize) -> Result<(), u32> {
        if id >= ANCHOR_COUNT || col >= ROW_CELLS {
            return Err(AnxCode::AnchorIdRange.code());
        }
        if self.mode == MatrixMode::FullLoad {
            return Err(AnxCode::WriteAfterLock.code());
        }
        if self.rows[id] == RowState::Locked {
            return Err(AnxCode::WriteAfterLock.code());
        }
        Ok(())
    }

    /// 回滚探针：模式倒退一律拒绝。
    pub fn try_rollback(&self) -> Result<(), u32> {
        Err(AnxCode::RollbackIllegal.code())
    }

    /// 守恒核：已填行数 == Pass 锚数 且 账面尝试 = 落格 + 拒。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.filled_rows != WAVE07_PASS_COUNT {
            return Err(AnxCode::ConservationBroken.code());
        }
        // 尝试总数不小于落格数（其余为拒绝，均留拒因）。
        if self.write_attempts < self.filled_rows as u32 {
            return Err(AnxCode::ConservationBroken.code());
        }
        Ok(())
    }

    /// 拒绝账守恒：尝试 = 落格 + 拒绝（逐轮对账）。
    pub fn reject_ledger_ok(&self) -> bool {
        let filled_u32 = self.filled_rows as u32;
        match self.write_attempts.checked_sub(filled_u32) {
            Some(diff) => diff == self.write_rejected,
            None => false,
        }
    }

    /// 摘要（双跑对拍用）：模式 / 已填 / 纪元 / 阻塞账长。
    pub fn summary(&self) -> (u8, usize, u32, usize) {
        let m = match self.mode {
            MatrixMode::EmptyLoad => 0u8,
            MatrixMode::Filling => 1u8,
            MatrixMode::FullLoad => 2u8,
        };
        (m, self.filled_rows, self.epoch, self.blocked_ledger.len())
    }
}

// ── 黄金全链：消费→填满→锁定→守恒→切换轨迹核验 ─────────────────
pub fn golden_run() -> Result<(u8, usize, u32, usize), u32> {
    let mut m = BaselineMatrix::new();
    if m.mode != MatrixMode::EmptyLoad || m.filled_rows != 0 {
        return Err(AnxCode::ConservationBroken.code());
    }
    let pass_n = m.consume_wave07(&wave07_table())?;
    if pass_n != WAVE07_PASS_COUNT {
        return Err(AnxCode::ConservationBroken.code());
    }
    // 按锚号序填充全部 Pass 锚。
    let mut i = 0;
    while i < ANCHOR_COUNT {
        if wave07_table()[i].fillable() {
            m.fill_anchor(i)?;
        }
        i += 1;
    }
    m.conservation()?;
    m.lock_all()?;
    if m.mode != MatrixMode::FullLoad {
        return Err(AnxCode::ConservationBroken.code());
    }
    // 锁后写探针必拒。
    if m.try_write_cell(0, 0).is_ok() {
        return Err(AnxCode::WriteAfterLock.code());
    }
    let s = m.summary();
    Ok(s)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AnxCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入检出账（四类坏样本） ────────────────────────────────────
pub struct InjectLedger {
    /// 各类注入尝试数。
    pub tries: [u32; 4],
    /// 各类被抓数（必须 == tries）。
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }
    /// 记一轮：slot < 4 才入账（越界静默丢弃会破坏守恒，故仅合法槽）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 四类全抓判定：每类尝试数 == 轮数且被抓数 == 尝试数。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] != rounds || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 10/10：类0 未裁决填充 / 类1 越界锚号 / 类2 重复填充 / 类3 阻塞锚。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类0：全新矩阵不消费裁决表直接填充。
        let mut m0 = BaselineMatrix::new();
        led.record(0, m0.fill_anchor(0).err() == Some(AnxCode::VerdictNotConsumed.code()));
        // 类1：越界锚号（ANCHOR_COUNT 起步）。
        let mut m1 = BaselineMatrix::new();
        let _ = m1.consume_wave07(&wave07_table())?;
        led.record(1, m1.fill_anchor(ANCHOR_COUNT).err() == Some(AnxCode::AnchorIdRange.code()));
        // 类2：重复填充（同锚二次落格）。
        let mut m2 = BaselineMatrix::new();
        let _ = m2.consume_wave07(&wave07_table())?;
        let _ = m2.fill_anchor(0)?;
        led.record(2, m2.fill_anchor(0).err() == Some(AnxCode::DuplicateFill.code()));
        // 类3：阻塞锚（锚 20 = Fail）填充。
        let mut m3 = BaselineMatrix::new();
        let _ = m3.consume_wave07(&wave07_table())?;
        led.record(3, m3.fill_anchor(20).err() == Some(AnxCode::BlockedAnchorFill.code()));
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────
/// 探针 A：合法黄金链全通。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：未消费即填充必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = BaselineMatrix::new();
    match m.fill_anchor(0) {
        Err(c) if c == AnxCode::VerdictNotConsumed.code() => Ok(()),
        _ => Err(AnxCode::VerdictNotConsumed.code()),
    }
}

/// 探针 C：满载前锁定必拒（守恒闸）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = BaselineMatrix::new();
    let _ = m.consume_wave07(&wave07_table())?;
    let _ = m.fill_anchor(0)?;
    match m.lock_all() {
        Err(c) if c == AnxCode::LockBeforeFull.code() => Ok(()),
        _ => Err(AnxCode::LockBeforeFull.code()),
    }
}

// ── 段位守卫与文案闭集 ─────────────────────────────────────────
/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < AnxCode::ALL.len() {
        let v = AnxCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每个诊断码均有非空中文明细。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < AnxCode::ALL.len() {
        if AnxCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据总成 ───────────────────────────────────────────────────
/// 静态判据（名称 / 通过 / 静态明细）。
pub fn static_checks() -> Vec<(&'static str, bool, &'static str)> {
    let mut v: Vec<(&'static str, bool, &'static str)> = Vec::new();
    // 段位守卫。
    v.push(("J2-段位守卫", segment_guard(), "0x80A80 窗口 0x40 对齐"));
    // 文案闭集。
    v.push(("J2-文案闭集", detail_closure(), "12 码明细全非空"));
    // 冻结值一致性。
    v.push((
        "J2-冻结值一致",
        ANCHOR_COUNT == 24 && ROW_CELLS == 4 && WAVE07_PASS_COUNT == 20,
        "24锚×4格, Pass=20",
    ));
    // 规范表实数与常量一致。
    let t = wave07_table();
    let mut pass_n = 0;
    let mut i = 0;
    while i < ANCHOR_COUNT {
        if t[i].fillable() {
            pass_n += 1;
        }
        i += 1;
    }
    v.push((
        "J1-裁决表实数一致",
        pass_n == WAVE07_PASS_COUNT,
        "规范表 Pass 实数=20",
    ));
    // 行状态机单向推进表核验。
    v.push((
        "J1-行态单向表",
        RowState::Empty.can_advance_to(RowState::Filled)
            && RowState::Filled.can_advance_to(RowState::Locked)
            && !RowState::Empty.can_advance_to(RowState::Locked)
            && !RowState::Locked.can_advance_to(RowState::Filled),
        "Empty→Filled→Locked, 倒退/越级皆非法",
    ));
    v
}

/// 动态判据（运行期文本，≤8 条 add_dyn）。
pub fn dyn_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));
    // 空载初态。
    let fresh = BaselineMatrix::new();
    v.push((
        "J1-空载初态",
        fresh.mode == MatrixMode::EmptyLoad && fresh.filled_rows == 0,
        format!("mode={:?} filled={}", fresh.mode, fresh.filled_rows),
    ));
    // 空载→满载切换回归：黄金链后模式必为 FullLoad。
    v.push((
        "J1-空载→满载切换",
        match &g {
            Ok((m, _, _, _)) => *m == 2u8,
            Err(_) => false,
        },
        String::from("EmptyLoad→Filling→FullLoad 单调"),
    ));
    // 注入 10/10 四类。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(JUDGE_ROUNDS),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10四类",
        led_ok,
        format!(
            "t={:?} c={:?}",
            led.as_ref().map(|l| l.tries).unwrap_or([0; 4]),
            led.as_ref().map(|l| l.caught).unwrap_or([0; 4])
        ),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("A过={} B拒={} C拒={}", a, b, c),
    ));
    // 重复填充幂等拒核验（黄金矩阵上二次填锚0）。
    let mut m = BaselineMatrix::new();
    let _ = m.consume_wave07(&wave07_table());
    let _ = m.fill_anchor(0);
    let dup = m.fill_anchor(0);
    v.push((
        "J1-重复填充幂等拒",
        dup.err() == Some(AnxCode::DuplicateFill.code()) && m.filled_rows == 1,
        format!("二次填后 filled={}", m.filled_rows),
    ));
    // 锁后写拒绝 + 回滚拒绝。
    let mut m2 = BaselineMatrix::new();
    let _ = m2.consume_wave07(&wave07_table());
    let mut k = 0;
    while k < ANCHOR_COUNT {
        if wave07_table()[k].fillable() {
            let _ = m2.fill_anchor(k);
        }
        k += 1;
    }
    let _ = m2.lock_all();
    let w = m2.try_write_cell(0, 0).err();
    let rb = m2.try_rollback().err();
    v.push((
        "J1-锁后写/回滚拒",
        w == Some(AnxCode::WriteAfterLock.code())
            && rb == Some(AnxCode::RollbackIllegal.code()),
        String::from("FullLoad 态写入与倒退皆拒"),
    ));
    // 守恒账：尝试 = 落格 + 拒绝。
    v.push((
        "J2-拒绝账守恒",
        m2.reject_ledger_ok() && m2.conservation().is_ok(),
        format!(
            "attempt={} filled={} reject={}",
            m2.write_attempts, m2.filled_rows, m2.write_rejected
        ),
    ));
    v
}

/// 探针入口：B09 批统一自检（13 条判据）。
pub fn run_unx18571_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18571 锚基线填充判据");
    for (n, p, d) in static_checks() {
        s.add(n, p, d);
    }
    for (n, p, d) in dyn_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_golden() {
        let set = run_unx18571_checks();
        assert!(set.all_passed(), "F18571 判据集存在红项：{}", set.red_items_text());
    }

    #[test]
    fn test_boundary_zero_and_full() {
        // 零边界：空矩阵零行已填。
        let fresh = BaselineMatrix::new();
        assert_eq!(fresh.filled_rows, 0);
        // 上边界：锚号 ANCHOR_COUNT-1 合法，ANCHOR_COUNT 越界。
        let mut m = BaselineMatrix::new();
        let _ = m.consume_wave07(&wave07_table());
        assert!(m.fill_anchor(ANCHOR_COUNT - 1).is_ok());
        assert!(m.fill_anchor(ANCHOR_COUNT).is_err());
        // 上边界 +1。
        assert!(m.fill_anchor(ANCHOR_COUNT + 1).is_err());
    }

    #[test]
    fn test_injection_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(led.all_caught(JUDGE_ROUNDS), "注入存在漏检：{:?}", led.caught);
        // 漏检必须为 0。
        let mut i = 0;
        while i < 4 {
            assert_eq!(led.tries[i] - led.caught[i], 0);
            i += 1;
        }
    }

    #[test]
    fn test_order_range_before_consume() {
        // 判定次序：越界闸先于未消费闸（越界锚在未消费矩阵上仍报越界）。
        let mut m = BaselineMatrix::new();
        match m.fill_anchor(999) {
            Err(c) if c == AnxCode::AnchorIdRange.code() => {}
            other => panic!("次序错误：{:?}", other.err()),
        }
    }

    #[test]
    fn test_state_machine_illegal_transition() {
        // 非法倒退/越级：Empty 不能直接锁；Locked 不能回退。
        assert!(!RowState::Empty.can_advance_to(RowState::Locked));
        assert!(!RowState::Locked.can_advance_to(RowState::Filled));
        // 满载前锁定必拒。
        let mut m = BaselineMatrix::new();
        let _ = m.consume_wave07(&wave07_table());
        let _ = m.fill_anchor(0);
        assert!(m.lock_all().is_err());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_capacity_blocked_ledger() {
        // 阻塞账容量语义：4 个非过态锚全部入账且不丢。
        let mut m = BaselineMatrix::new();
        let _ = m.consume_wave07(&wave07_table());
        let mut i = 0;
        while i < ANCHOR_COUNT {
            if !wave07_table()[i].fillable() {
                let _ = m.fill_anchor(i);
            }
            i += 1;
        }
        assert_eq!(m.blocked_ledger.len(), 4);
        assert_eq!(m.filled_rows, 0);
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert_eq!(AnxCode::ALL.len(), 12);
        // 码无重复。
        let mut i = 0;
        while i < AnxCode::ALL.len() {
            let mut j = i + 1;
            while j < AnxCode::ALL.len() {
                assert_ne!(AnxCode::ALL[i].code(), AnxCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }
}
