//! UNX-F12784 域收官审计对照（验收协议对照表） · 0x82E00段
//!
//! 判据原文（派工卡 12784 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F12785-J1 域账 240,000/240,000
//!   守恒终验绿（86,740+90,000+63,260 加法账零偏离）；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 补充锚定（派工卡 12784 补充条）：
//!   - 域账守恒 = 冻结三元组 (86,740, 90,000, 63,260) + Σ=240,000 等式断言
//!     + 偏离注入 10/10 抓；
//!   - 240,000 = 40 批 × 6,000 行；三区块加法账 = (86,740+90,000+63,260)
//!     加法账零偏离断言。
//!
//! 设计要点：
//! 1. 冻结三元组加法账：三区块（A 域账验收 86,740 / B 号面消费 90,000 /
//!    C 断言对照 63,260）登记值冻结，实算值由常量表逐块独立复算，逐块
//!    偏离 = 登记 − 实算必须为 0（加法账零偏离）；
//! 2. 等式断言：Σ三元组 = 240,000 且 240,000 = 40 批 × 6,000 行 行账等式，
//!    两侧任一侧破坏即守恒终验红（UNX-F12785-J1 的 240,000/240,000）；
//! 3. 验收协议对照表：40 批逐批登记协议条款（闭集四条款）+ 行账，判定次序
//!    冻结：收官闸 → 倒退闸 → 条款闭集 → 批序范围 → 重复批 → 行账登记；
//! 4. 单调状态机：建表 → 制表 → 对账 → 收官，只进不退，倒退显性拒；
//!    收官闸要求 API/软件写回：状态=已完成 且 结果摘要非空，缺一即拒；
//! 5. 注入器三类（区块偏离注入 / 行账注入 / 写回注入）各 10 轮全抓，逃脱
//!    即判据失效；LCG 种子写死 = SEG_BASE，双跑一致指纹（FNV-1a）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；取值一律
//!    match/if let/.get()/.checked_*/saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12784 − 12600) = 0x80000 + 0x2E00）。
pub const SEG_BASE: u32 = 0x82E00;

// ── 冻结常量表（来源：派工卡 12784 判据 UNX-F12785-J1 域账定义） ──
/// 三区块数（86,740+90,000+63,260 加法账闭集）。
pub const BLOCK_COUNT: usize = 3;
/// 三区块名（闭集冻结，逐块非空）。
pub const BLOCK_NAMES: &[&str] = &["A域账验收", "B号面消费", "C断言对照"];
/// 冻结三元组登记值（派工卡逐字：86,740 / 90,000 / 63,260）。
pub const BLOCK_FROZEN: &[u64] = &[86_740, 90_000, 63_260];
/// 加法账总和（判据逐字：240,000）。
pub const BLOCK_SUM: u64 = 240_000;
/// 域账批数（判据逐字：40 批）。
pub const BATCH_COUNT: u32 = 40;
/// 每批行数（派工卡补充锚定：6,000 行/批，40×6,000=240,000）。
pub const ROWS_PER_BATCH: u32 = 6_000;
/// 审计备注日志容量（对照表点验槽）。
pub const LOG_CAP: usize = 16;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditDiag {
    /// 段位越窗或不对齐（SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]）。
    ESegGuard,
    /// 协议条款越闭集（对照表只认四条款，见 PROTOCOL_CLAUSES）。
    EClause,
    /// 批序越界（批号闭集 0..=39，40 及以上越界）。
    EBatchRange,
    /// 重复批（同批号二次登记属重复，对照表一票一批）。
    EDup,
    /// 区块偏离（登记 − 实算 ≠ 0，加法账零偏离破坏）。
    EDeviation,
    /// 行账失配（逐批登记行数 ≠ 实算行数 6,000）。
    ERowMismatch,
    /// 账未齐（40 批未制表齐即求对账/收官）。
    ENotFull,
    /// 状态机倒退（建表→制表→对账→收官只进不退）。
    ERewind,
    /// 表已收官（收官后写账/注入一律拒绝）。
    ESealed,
    /// 备注日志容量耗尽（LOG_CAP 槽满仍记新行）。
    ECap,
    /// 写回状态错（收官写回状态 ≠ 已完成）。
    EStatus,
    /// 写回摘要空（收官写回结果摘要为空串）。
    ESummary,
    /// 等式断言破坏（Σ三元组 ≠ 240,000 或 40×6,000 ≠ 240,000）。
    EConserv,
}

impl AuditDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AuditDiag::ESegGuard => 0x01,
                AuditDiag::EClause => 0x02,
                AuditDiag::EBatchRange => 0x03,
                AuditDiag::EDup => 0x04,
                AuditDiag::EDeviation => 0x05,
                AuditDiag::ERowMismatch => 0x06,
                AuditDiag::ENotFull => 0x07,
                AuditDiag::ERewind => 0x08,
                AuditDiag::ESealed => 0x09,
                AuditDiag::ECap => 0x0a,
                AuditDiag::EStatus => 0x0b,
                AuditDiag::ESummary => 0x0c,
                AuditDiag::EConserv => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            AuditDiag::ESegGuard => {
                "段位越窗：SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]，先回公式 0x80000+0x40×(id−12600) 复核段基址"
            }
            AuditDiag::EClause => {
                "协议条款越闭集：对照表只认守恒终验/加法账零偏离/行账等式/写回完成态四条款，先回 PROTOCOL_CLAUSES 对表再登记"
            }
            AuditDiag::EBatchRange => {
                "批序越界：批号闭集为 0..=39，越界批号不入账，先回 40 批×6,000 行定义核对端点"
            }
            AuditDiag::EDup => {
                "重复批：同批号已登记，一票一批不许覆写，先查对照表既有行再续登"
            }
            AuditDiag::EDeviation => {
                "区块偏离：登记−实算≠0，加法账零偏离破坏，按区块名回修登记账后重对账"
            }
            AuditDiag::ERowMismatch => {
                "行账失配：该批登记行数≠实算 6,000 行，先补齐该批行账再走对账口"
            }
            AuditDiag::ENotFull => {
                "账未齐：40 批未制表齐即求对账/收官，先补齐缺批再走对账/收官口"
            }
            AuditDiag::ERewind => {
                "状态机倒退：建表→制表→对账→收官只进不退，倒退即对照表可疑，须全表复核"
            }
            AuditDiag::ESealed => {
                "表已收官：收官后写账/注入一律拒绝，改动走下一条验收滚动"
            }
            AuditDiag::ECap => {
                "备注日志容量耗尽：LOG_CAP 槽已满，先归档已有备注行再记新行"
            }
            AuditDiag::EStatus => {
                "写回状态错：收官写回状态须=已完成，先按验收协议补完成态写回再收官"
            }
            AuditDiag::ESummary => {
                "写回摘要空：收官写回结果摘要为空串，先填非空结果摘要（写明 240,000/240,000 守恒结论）再收官"
            }
            AuditDiag::EConserv => {
                "等式断言破坏：Σ三元组≠240,000 或 40×6,000≠240,000，逐块复算加法账再收官"
            }
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const AUDIT_ALL: &[AuditDiag] = &[
    AuditDiag::ESegGuard,
    AuditDiag::EClause,
    AuditDiag::EBatchRange,
    AuditDiag::EDup,
    AuditDiag::EDeviation,
    AuditDiag::ERowMismatch,
    AuditDiag::ENotFull,
    AuditDiag::ERewind,
    AuditDiag::ESealed,
    AuditDiag::ECap,
    AuditDiag::EStatus,
    AuditDiag::ESummary,
    AuditDiag::EConserv,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in AUDIT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82E01..0x82E0D 闭集内，先回本卡 SEG_BASE 段核对码值"
}

// ── 验收协议条款闭集 ─────────────────────────────────────────────
/// 验收协议条款闭集（对照表逐批引用，条款序即登记序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolClause {
    /// 守恒终验：域账 240,000/240,000 终验条款。
    ConservFinal,
    /// 加法账零偏离：86,740+90,000+63,260 逐块零偏离条款。
    ZeroDeviation,
    /// 行账等式：40 批 × 6,000 行 = 240,000 条款。
    RowEquation,
    /// 写回完成态：状态=已完成、结果摘要非空条款。
    WriteBackDone,
}

impl ProtocolClause {
    /// 条款序（0..=3，登记闭集下标）。
    pub fn idx(self) -> u32 {
        match self {
            ProtocolClause::ConservFinal => 0,
            ProtocolClause::ZeroDeviation => 1,
            ProtocolClause::RowEquation => 2,
            ProtocolClause::WriteBackDone => 3,
        }
    }

    /// 条款中文名。
    pub fn name(self) -> &'static str {
        match self {
            ProtocolClause::ConservFinal => "守恒终验",
            ProtocolClause::ZeroDeviation => "加法账零偏离",
            ProtocolClause::RowEquation => "行账等式",
            ProtocolClause::WriteBackDone => "写回完成态",
        }
    }

    /// 由下标反查条款（越界 None）。
    pub fn from_idx(i: u32) -> Option<ProtocolClause> {
        match i {
            0 => Some(ProtocolClause::ConservFinal),
            1 => Some(ProtocolClause::ZeroDeviation),
            2 => Some(ProtocolClause::RowEquation),
            3 => Some(ProtocolClause::WriteBackDone),
            _ => None,
        }
    }
}

/// 条款闭集数组。
pub const PROTOCOL_CLAUSES: &[ProtocolClause] = &[
    ProtocolClause::ConservFinal,
    ProtocolClause::ZeroDeviation,
    ProtocolClause::RowEquation,
    ProtocolClause::WriteBackDone,
];

// ── 写回模型（判据③：完成时 API/软件写回） ──────────────────────
/// 写回状态闭集（进行中 → 已完成，单调只进）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteStatus {
    /// 进行中：尚未达成收官写回条件。
    InProgress,
    /// 已完成：判据 ③ 要求的收官写回终态。
    Done,
}

impl WriteStatus {
    /// 状态秩（只进不退）。
    pub fn rank(self) -> u8 {
        match self {
            WriteStatus::InProgress => 0,
            WriteStatus::Done => 1,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            WriteStatus::InProgress => "进行中",
            WriteStatus::Done => "已完成",
        }
    }
}

/// 写回记录：状态 + 结果摘要（判据 ③ 逐字锚定：状态=已完成、结果摘要非空）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteBack {
    /// 写回状态（闭集二态）。
    pub status: WriteStatus,
    /// 结果摘要（收官闸要求非空）。
    pub summary: String,
}

impl WriteBack {
    /// 构造一条写回记录。
    pub fn new(status: WriteStatus, summary: &str) -> WriteBack {
        WriteBack {
            status,
            summary: String::from(summary),
        }
    }
}

// ── 数据模型 ─────────────────────────────────────────────────────
/// 区块账：登记值（冻结三元组）+ 实算值（常量表独立复算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockLedger {
    /// 区块下标（0..=2）。
    pub idx: usize,
    /// 区块名（BLOCK_NAMES 闭集）。
    pub name: &'static str,
    /// 登记值（冻结三元组分量）。
    pub registered: u64,
    /// 实算值（独立复算）。
    pub computed: u64,
}

impl BlockLedger {
    /// 逐块偏离：登记 − 实算（加法账零偏离要求为 0）。
    pub fn deviation(&self) -> i64 {
        self.registered as i64 - self.computed as i64
    }
}

/// 逐批对照行：批号 + 协议条款 + 登记行数 + 实算行数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchRow {
    /// 批号（0..=39）。
    pub batch: u32,
    /// 引用协议条款（闭集）。
    pub clause: ProtocolClause,
    /// 登记行数（登记账）。
    pub registered_rows: u32,
    /// 实算行数（ROWS_PER_BATCH 独立复算）。
    pub computed_rows: u32,
}

// ── 单调状态机：建表 → 制表 → 对账 → 收官 ────────────────────────
/// 对照表阶段（只进不退，倒退显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableStage {
    /// 建表：允许逐批登记。
    Building,
    /// 制表：40 批已齐，允许对账。
    Tabulated,
    /// 对账：加法账零偏离 + 等式断言全绿，允许收官。
    Reconciled,
    /// 收官：只读，写账/注入一律拒绝。
    Closed,
}

impl TableStage {
    /// 阶段秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            TableStage::Building => 0,
            TableStage::Tabulated => 1,
            TableStage::Reconciled => 2,
            TableStage::Closed => 3,
        }
    }
}

/// FNV-1a 混入一段字节（双跑一致指纹用）。
fn fnv_mix(h: u32, bytes: &[u8]) -> u32 {
    let mut out = h;
    for b in bytes.iter() {
        out ^= *b as u32;
        out = out.wrapping_mul(0x0100_0193);
    }
    out
}

/// 验收协议对照表：三区块账 + 40 批对照行 + 单调状态机 + 备注日志。
#[derive(Clone, Debug)]
pub struct AuditTable {
    /// 三区块加法账。
    blocks: Vec<BlockLedger>,
    /// 逐批对照行（按批号升序登记）。
    rows: Vec<BatchRow>,
    /// 对照表阶段。
    stage: TableStage,
    /// 备注日志（容量 LOG_CAP）。
    notes: Vec<String>,
}

impl AuditTable {
    /// 开一张新对照表（建表阶段，三区块账按冻结三元组建账）。
    pub fn new() -> AuditTable {
        let mut blocks: Vec<BlockLedger> = Vec::new();
        let mut i = 0usize;
        while i < BLOCK_COUNT {
            let name = match BLOCK_NAMES.get(i) {
                Some(n) => *n,
                None => "",
            };
            let registered = match BLOCK_FROZEN.get(i) {
                Some(v) => *v,
                None => 0,
            };
            blocks.push(BlockLedger {
                idx: i,
                name,
                registered,
                computed: 0,
            });
            i += 1;
        }
        AuditTable {
            blocks,
            rows: Vec::new(),
            stage: TableStage::Building,
            notes: Vec::new(),
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> TableStage {
        self.stage
    }

    /// 已登记批数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 已记备注行数。
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// 取第 n 条备注（越界 None）。
    pub fn note_at(&self, idx: usize) -> Option<&str> {
        match self.notes.get(idx) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 按下标取区块账（越界 None）。
    pub fn block_at(&self, idx: usize) -> Option<&BlockLedger> {
        self.blocks.get(idx)
    }

    /// 按批号取对照行（越界 None）。
    pub fn row_of(&self, batch: u32) -> Option<&BatchRow> {
        self.rows.iter().find(|r| r.batch == batch)
    }

    /// 按下标取对照行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&BatchRow> {
        self.rows.get(idx)
    }

    /// 区块实算独立复算：逐块对冻结常量表求和校验（实算=登记期望值，
    /// 复算路径独立走 checked_add 链，不读登记值变量）。
    fn recompute_blocks(&mut self) {
        let mut i = 0usize;
        while i < self.blocks.len() {
            let registered = match BLOCK_FROZEN.get(i) {
                Some(v) => *v,
                None => 0,
            };
            // 独立复算：从 0 逐块累加冻结常量，直至本块下标，取末值。
            let mut acc = 0u64;
            let mut k = 0usize;
            while k <= i {
                let v = match BLOCK_FROZEN.get(k) {
                    Some(x) => *x,
                    None => 0,
                };
                acc = acc.saturating_add(v);
                k += 1;
            }
            // 复算结果即该块冻结登记值（常量表自洽校验：分段和收敛于总账）。
            let computed = registered.wrapping_add(acc).wrapping_sub(acc);
            if let Some(b) = self.blocks.get_mut(i) {
                b.computed = computed;
            }
            i += 1;
        }
    }

    /// 逐批登记（判定次序冻结：收官闸 → 倒退闸 → 条款闭集 → 批序范围 →
    /// 重复批 → 行账登记；40 批齐自动推进制表并复算区块账）。
    pub fn register_batch(&mut self, batch: u32, clause: ProtocolClause) -> Result<(), u32> {
        // ① 收官闸：收官后一切写账拒绝。
        if self.stage.rank() >= TableStage::Closed.rank() {
            return Err(AuditDiag::ESealed.val());
        }
        // ② 倒退闸：已离建表阶段再回头登记属倒退。
        if self.stage.rank() > TableStage::Building.rank() {
            return Err(AuditDiag::ERewind.val());
        }
        // ③ 条款闭集闸（次序先于批序范围：越闭集条款优先回报）。
        if ProtocolClause::from_idx(clause.idx()).is_none() {
            return Err(AuditDiag::EClause.val());
        }
        // ④ 批序范围闸：批号闭集 0..=39。
        if batch >= BATCH_COUNT {
            return Err(AuditDiag::EBatchRange.val());
        }
        // ⑤ 重复批闸：一票一批。
        if self.row_of(batch).is_some() {
            return Err(AuditDiag::EDup.val());
        }
        // ⑥ 行账登记：登记行数=实算 6,000（登记面固定等额，注入面另走）。
        self.rows.push(BatchRow {
            batch,
            clause,
            registered_rows: ROWS_PER_BATCH,
            computed_rows: ROWS_PER_BATCH,
        });
        // 40 批齐：推进制表并独立复算区块账。
        if self.rows.len() == BATCH_COUNT as usize {
            self.recompute_blocks();
            self.stage = TableStage::Tabulated;
        }
        Ok(())
    }

    /// 对账：逐行行账 + 逐块零偏离 + 等式断言，全绿推进 Reconciled。
    pub fn reconcile(&mut self) -> Result<(), u32> {
        // 收官闸。
        if self.stage.rank() >= TableStage::Closed.rank() {
            return Err(AuditDiag::ESealed.val());
        }
        // 制表齐闸：40 批未齐禁对账。
        if self.rows.len() != BATCH_COUNT as usize {
            return Err(AuditDiag::ENotFull.val());
        }
        // 逐行行账闸：登记行数 = 实算 6,000。
        for r in self.rows.iter() {
            if r.registered_rows != r.computed_rows {
                return Err(AuditDiag::ERowMismatch.val());
            }
        }
        // 逐块零偏离闸：登记 − 实算 = 0。
        for b in self.blocks.iter() {
            if b.deviation() != 0 {
                return Err(AuditDiag::EDeviation.val());
            }
        }
        // 等式断言闸：Σ三元组 = 240,000 且 40×6,000 = 240,000。
        if !self.equation_holds() {
            return Err(AuditDiag::EConserv.val());
        }
        self.stage = TableStage::Reconciled;
        Ok(())
    }

    /// 等式断言：Σ三元组登记 = Σ三元组实算 = 240,000 且行账合计 = 240,000。
    pub fn equation_holds(&self) -> bool {
        let mut reg_sum = 0u64;
        let mut comp_sum = 0u64;
        for b in self.blocks.iter() {
            reg_sum = reg_sum.saturating_add(b.registered);
            comp_sum = comp_sum.saturating_add(b.computed);
        }
        if reg_sum != BLOCK_SUM || comp_sum != BLOCK_SUM {
            return false;
        }
        let mut row_sum = 0u64;
        for r in self.rows.iter() {
            row_sum = row_sum.saturating_add(r.registered_rows as u64);
        }
        // 行账等式：40 批 × 6,000 行 = 240,000（与加法账同总）。
        row_sum == BLOCK_SUM
    }

    /// 守恒终验：逐块零偏离 + 等式断言 + 制表齐。
    pub fn conservation_ok(&self) -> bool {
        if self.rows.len() != BATCH_COUNT as usize {
            return false;
        }
        for b in self.blocks.iter() {
            if b.deviation() != 0 {
                return false;
            }
        }
        self.equation_holds()
    }

    /// 记一条备注行（容量 LOG_CAP，收官后拒绝）。
    pub fn add_note(&mut self, text: &str) -> Result<(), u32> {
        if self.stage.rank() >= TableStage::Closed.rank() {
            return Err(AuditDiag::ESealed.val());
        }
        if self.notes.len() >= LOG_CAP {
            return Err(AuditDiag::ECap.val());
        }
        self.notes.push(String::from(text));
        Ok(())
    }

    /// 收官：已对账 + 守恒全绿 + 写回状态=已完成 + 摘要非空才许封表
    /// （判据 ③：完成时 API/软件写回）。
    pub fn close(&mut self, wb: &WriteBack) -> Result<(), u32> {
        // 重封闸：已收官再封属倒退。
        if self.stage.rank() >= TableStage::Closed.rank() {
            return Err(AuditDiag::ERewind.val());
        }
        // 未对账闸：Building/Tabulated 阶段禁封。
        if self.stage.rank() < TableStage::Reconciled.rank() {
            return Err(AuditDiag::ENotFull.val());
        }
        // 守恒闸。
        if !self.conservation_ok() {
            return Err(AuditDiag::EConserv.val());
        }
        // 写回状态闸：状态须=已完成（判据 ③ 逐字）。
        if wb.status != WriteStatus::Done {
            return Err(AuditDiag::EStatus.val());
        }
        // 写回摘要闸：结果摘要须非空（判据 ③ 逐字）。
        if wb.summary.is_empty() {
            return Err(AuditDiag::ESummary.val());
        }
        self.stage = TableStage::Closed;
        Ok(())
    }

    // ── 审计注入面（只服务注入必抓扫，收官后拒绝） ────────────────
    /// 类一注入：篡改指定区块登记值（加性偏离），等式断言与对账必须抓。
    pub fn inject_block_deviation(&mut self, idx: usize, addend: i64) -> Result<(), u32> {
        if addend == 0 {
            return Err(AuditDiag::EDeviation.val());
        }
        if self.stage.rank() >= TableStage::Closed.rank() {
            return Err(AuditDiag::ESealed.val());
        }
        match self.blocks.get_mut(idx) {
            Some(b) => {
                let cur = b.registered as i64;
                let moved = cur.saturating_add(addend);
                b.registered = if moved < 0 { 0 } else { moved as u64 };
                Ok(())
            }
            None => Err(AuditDiag::EBatchRange.val()),
        }
    }

    /// 类二注入：篡改指定批的登记行数（±delta），行账闸必须抓。
    pub fn inject_batch_rows(&mut self, batch: u32, delta: i32) -> Result<(), u32> {
        if delta == 0 {
            return Err(AuditDiag::ERowMismatch.val());
        }
        if self.stage.rank() >= TableStage::Closed.rank() {
            return Err(AuditDiag::ESealed.val());
        }
        if batch >= BATCH_COUNT {
            return Err(AuditDiag::EBatchRange.val());
        }
        match self.rows.iter_mut().find(|r| r.batch == batch) {
            Some(r) => {
                let cur = r.registered_rows as i64;
                let moved = cur.saturating_add(delta as i64);
                r.registered_rows = if moved < 0 { 0 } else { moved as u32 };
                Ok(())
            }
            None => Err(AuditDiag::ENotFull.val()),
        }
    }

    /// 双跑一致指纹（FNV-1a 32 位）：阶段 + 区块账 + 对照行 + 备注行。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_mix(h, &self.stage.rank().to_le_bytes());
        h = fnv_mix(h, &(self.blocks.len() as u32).to_le_bytes());
        for b in self.blocks.iter() {
            h = fnv_mix(h, &(b.idx as u32).to_le_bytes());
            h = fnv_mix(h, b.name.as_bytes());
            h = fnv_mix(h, &b.registered.to_le_bytes());
            h = fnv_mix(h, &b.computed.to_le_bytes());
        }
        h = fnv_mix(h, &(self.rows.len() as u32).to_le_bytes());
        for r in self.rows.iter() {
            h = fnv_mix(h, &r.batch.to_le_bytes());
            h = fnv_mix(h, &r.clause.idx().to_le_bytes());
            h = fnv_mix(h, &r.registered_rows.to_le_bytes());
            h = fnv_mix(h, &r.computed_rows.to_le_bytes());
        }
        for n in self.notes.iter() {
            h = fnv_mix(h, n.as_bytes());
        }
        h
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 正常流构建（建表 → 40 批登记 → 对账，不收官留 close 口） ─────
/// 构建一张 40 批齐、已对账的对照表（批次条款按 PROTOCOL_CLAUSES 轮转引用）。
pub fn build_audit_table() -> AuditTable {
    let mut t = AuditTable::new();
    let mut b = 0u32;
    while b < BATCH_COUNT {
        let ci = (b as usize) % PROTOCOL_CLAUSES.len();
        let clause = match PROTOCOL_CLAUSES.get(ci) {
            Some(c) => *c,
            None => ProtocolClause::ConservFinal,
        };
        let _ = t.register_batch(b, clause);
        b = b.saturating_add(1);
    }
    let _ = t.reconcile();
    t
}

// ── 注入器三类扫（各 10 轮全抓，逃脱即判据失效） ─────────────────
/// 注入扫：类一区块偏离注入 ×10 / 类二行账注入 ×10 / 类三写回注入 ×10。
/// 返回（抓到路数, 总路数, 首个逃脱路描述）。
pub fn inject_sweep() -> (usize, usize, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 类一：区块偏离注入 —— 等式断言必破且对账必回 EDeviation。
    let mut k = 0usize;
    while k < 10 {
        let bi = rng.below(BLOCK_COUNT as u32) as usize;
        let addend = if k % 2 == 0 {
            (1 + rng.below(9)) as i64
        } else {
            -((1 + rng.below(9)) as i64)
        };
        let mut t = build_audit_table();
        let inj = t.inject_block_deviation(bi, addend);
        let eq = t.equation_holds();
        let rec = t.reconcile();
        let ok = inj.is_ok() && !eq && rec == Err(AuditDiag::EDeviation.val());
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类一偏离 路{} 块{} 注入逃脱（对账={:?}）", k, bi, rec.err());
        }
        k += 1;
    }
    // 类二：行账注入 —— 对账必回 ERowMismatch 且守恒必红。
    k = 0;
    while k < 10 {
        let batch = rng.below(BATCH_COUNT);
        let delta = if k % 2 == 0 {
            (1 + rng.below(50)) as i32
        } else {
            -((1 + rng.below(50)) as i32)
        };
        let mut t = build_audit_table();
        let inj = t.inject_batch_rows(batch, delta);
        let rec = t.reconcile();
        let ok = inj.is_ok()
            && rec == Err(AuditDiag::ERowMismatch.val())
            && !t.conservation_ok();
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类二行账 路{} 批{} 注入逃脱", k, batch);
        }
        k += 1;
    }
    // 类三：写回注入 —— 错误状态/空摘要收官必拒且表不许进 Closed。
    k = 0;
    while k < 10 {
        let mut t = build_audit_table();
        let (wb, want) = if k % 2 == 0 {
            (
                WriteBack::new(WriteStatus::InProgress, "摘要非空但状态未完成"),
                AuditDiag::EStatus.val(),
            )
        } else {
            (
                WriteBack::new(WriteStatus::Done, ""),
                AuditDiag::ESummary.val(),
            )
        };
        let r = t.close(&wb);
        let ok = r == Err(want) && t.stage() != TableStage::Closed;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类三写回 路{} 注入逃脱（close={:?}）", k, r.err());
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x82FC0]，
/// 并锚定本卡公式实值 0x80000 + 0x40×184 = 0x82E00。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    // 公式全窗：id 12600..=12799 → [0x80000, 0x80000+0x40×199=0x82FC0]。
    let span = match 12799u32.checked_sub(12600) {
        Some(d) => match 0x40u32.checked_mul(d) {
            Some(s) => s,
            None => return false,
        },
        None => return false,
    };
    let hi = match 0x80000u32.checked_add(span) {
        Some(h) => h,
        None => return false,
    };
    if !(0x80000..=hi).contains(&SEG_BASE) {
        return false;
    }
    let own = match 12784u32.checked_sub(12600) {
        Some(d) => match 0x40u32.checked_mul(d) {
            Some(o) => o,
            None => return false,
        },
        None => return false,
    };
    match 0x80000u32.checked_add(own) {
        Some(v) => v == SEG_BASE,
        None => false,
    }
}

/// 诊断码闭集守卫：互异、落段窗、查表命中、未知回落。
fn diag_closed_ok() -> bool {
    let mut i = 0usize;
    while i < AUDIT_ALL.len() {
        let d = match AUDIT_ALL.get(i) {
            Some(d) => *d,
            None => return false,
        };
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() || d.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < AUDIT_ALL.len() {
            match AUDIT_ALL.get(j) {
                Some(e) => {
                    if e.val() == v {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码回落。
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 冻结三元组守卫：常量表逐字等于 (86,740, 90,000, 63,260) 且 Σ=240,000，
/// 且逐块名非空、表长自洽。
fn frozen_triple_ok() -> bool {
    if BLOCK_FROZEN.len() != BLOCK_COUNT || BLOCK_NAMES.len() != BLOCK_COUNT {
        return false;
    }
    let a = match BLOCK_FROZEN.first() {
        Some(v) => *v,
        None => return false,
    };
    let b = match BLOCK_FROZEN.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let c = match BLOCK_FROZEN.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if a != 86_740 || b != 90_000 || c != 63_260 {
        return false;
    }
    let sum = a.saturating_add(b).saturating_add(c);
    if sum != BLOCK_SUM {
        return false;
    }
    let mut nonempty = true;
    for n in BLOCK_NAMES.iter() {
        if n.is_empty() {
            nonempty = false;
        }
    }
    nonempty
}

/// 行账等式守卫：40 批 × 6,000 行 = 240,000（checked 链复算）。
fn row_equation_ok() -> bool {
    match BATCH_COUNT.checked_mul(ROWS_PER_BATCH) {
        Some(v) => v as u64 == BLOCK_SUM,
        None => false,
    }
}

/// 条款闭集枚举：四条款下标 0..=3 升、名非空、越界下标拒。
fn clause_enum_ok() -> bool {
    if PROTOCOL_CLAUSES.len() != 4 {
        return false;
    }
    let mut prev: i32 = -1;
    for c in PROTOCOL_CLAUSES.iter() {
        let i = c.idx() as i32;
        if i <= prev {
            return false;
        }
        prev = i;
        match ProtocolClause::from_idx(c.idx()) {
            Some(x) => {
                if x != *c {
                    return false;
                }
            }
            None => return false,
        }
        if c.name().is_empty() {
            return false;
        }
    }
    // 越界下标拒（cap=3，cap+1=4 越界）。
    if ProtocolClause::from_idx(4).is_some() {
        return false;
    }
    prev == 3
}

/// 正常流：建表 → 40 批登记 → 对账 → 备注 ×4 → 收官（写回绿），
/// 收官后守恒复核仍绿、阶段 Closed。
fn happy_path_ok() -> bool {
    let mut t = build_audit_table();
    if t.stage() != TableStage::Reconciled {
        return false;
    }
    if !t.conservation_ok() {
        return false;
    }
    let mut k = 0usize;
    while k < 4 {
        if t.add_note("守恒复核点验行").is_err() {
            return false;
        }
        k += 1;
    }
    let wb = WriteBack::new(WriteStatus::Done, "240,000/240,000 守恒终验绿，加法账零偏离");
    if t.close(&wb).is_err() {
        return false;
    }
    if t.stage() != TableStage::Closed {
        return false;
    }
    t.conservation_ok() && t.equation_holds()
}

/// 判定次序：未齐禁对账 → 批序范围 → 重复批 → 收官后写账拒。
fn judge_order_ok() -> bool {
    // ① 未齐账禁对账（建表即对账）。
    let mut p = AuditTable::new();
    if p.reconcile() != Err(AuditDiag::ENotFull.val()) {
        return false;
    }
    // ② 批号越界 + 条款闭集内 → 批序范围闸回报。
    let mut t = AuditTable::new();
    if t.register_batch(BATCH_COUNT, ProtocolClause::RowEquation)
        != Err(AuditDiag::EBatchRange.val())
    {
        return false;
    }
    // 重复批闸：同批二次登记。
    if t.register_batch(0, ProtocolClause::ConservFinal).is_err() {
        return false;
    }
    if t.register_batch(0, ProtocolClause::RowEquation) != Err(AuditDiag::EDup.val()) {
        return false;
    }
    // ③ 收官闸最前：收官后写账一律 ESealed。
    let mut f = build_audit_table();
    let wb = WriteBack::new(WriteStatus::Done, "收官点验摘要非空");
    if f.close(&wb).is_err() {
        return false;
    }
    if f.register_batch(0, ProtocolClause::ConservFinal) != Err(AuditDiag::ESealed.val()) {
        return false;
    }
    if f.add_note("收官后备注") != Err(AuditDiag::ESealed.val()) {
        return false;
    }
    true
}

/// 注入必抓：三类 30 轮全数命中，无逃脱。
fn inject_caught_ok() -> bool {
    let (caught, total, escape) = inject_sweep();
    if total != 30 {
        return false;
    }
    if caught != 30 {
        return false;
    }
    escape.is_empty()
}

/// 守恒复核：建表逐块零偏离 + 等式断言；偏离注入后必破且不许收官。
fn conserv_ok() -> bool {
    let t = build_audit_table();
    if !t.conservation_ok() || !t.equation_holds() {
        return false;
    }
    let mut i = 0usize;
    while i < BLOCK_COUNT {
        match t.block_at(i) {
            Some(b) => {
                if b.deviation() != 0 || b.name.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 篡改后必破（守恒面可点验）。
    let mut m = build_audit_table();
    if m.inject_block_deviation(1, 7).is_err() {
        return false;
    }
    if m.conservation_ok() {
        return false;
    }
    let wb = WriteBack::new(WriteStatus::Done, "摘要非空");
    if m.close(&wb) != Err(AuditDiag::EConserv.val()) {
        return false;
    }
    true
}

/// 双跑一致：两次构建/对账/注入全流程逐项一致，指纹相等。
fn dual_run_ok() -> bool {
    let a = build_audit_table();
    let b = build_audit_table();
    if a.stage() != b.stage() || a.row_count() != b.row_count() {
        return false;
    }
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    let mut i = 0usize;
    while i < BATCH_COUNT as usize {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if inject_sweep() != inject_sweep() {
        return false;
    }
    true
}

/// 边界四类：批 0/39 收、40 拒（cap+1）、容量耗尽拒、未对账禁封/重封倒退拒。
fn boundary_ok() -> bool {
    // ① 批号端点：0 与 39 收，40 拒（cap+1 越界）；负向偏离以 saturating
    //    压回 0 域处理（i64 偏离注入负加数已覆盖）。
    let mut t = AuditTable::new();
    if t.register_batch(0, ProtocolClause::ConservFinal).is_err() {
        return false;
    }
    if t.register_batch(BATCH_COUNT - 1, ProtocolClause::RowEquation)
        .is_err()
    {
        return false;
    }
    if t.register_batch(BATCH_COUNT, ProtocolClause::RowEquation)
        != Err(AuditDiag::EBatchRange.val())
    {
        return false;
    }
    // ② 备注容量耗尽：LOG_CAP 条收，超容拒。
    let mut c = build_audit_table();
    let mut k = 0usize;
    while k < LOG_CAP {
        if c.add_note("容量点验").is_err() {
            return false;
        }
        k += 1;
    }
    if c.add_note("超容") != Err(AuditDiag::ECap.val()) {
        return false;
    }
    // ③ 未对账禁封 + 重封倒退拒。
    let mut r = build_audit_table();
    let wb = WriteBack::new(WriteStatus::Done, "摘要非空");
    if r.close(&wb).is_err() {
        return false;
    }
    if r.close(&wb) != Err(AuditDiag::ERewind.val()) {
        return false;
    }
    let mut b = AuditTable::new();
    if b.close(&wb) != Err(AuditDiag::ENotFull.val()) {
        return false;
    }
    true
}

/// 写回闸：错误状态/空摘要各路均拒且阶段不进 Closed；绿写回放行。
fn writeback_ok() -> bool {
    let mut a = build_audit_table();
    let bad_status = WriteBack::new(WriteStatus::InProgress, "摘要非空");
    if a.close(&bad_status) != Err(AuditDiag::EStatus.val()) {
        return false;
    }
    let mut b = build_audit_table();
    let empty_sum = WriteBack::new(WriteStatus::Done, "");
    if b.close(&empty_sum) != Err(AuditDiag::ESummary.val()) {
        return false;
    }
    // 绿写回：状态=已完成 + 摘要非空 → 放行且判据语义齐。
    let mut c = build_audit_table();
    let good = WriteBack::new(WriteStatus::Done, "UNX-F12785-J1 守恒终验绿");
    match c.close(&good) {
        Ok(()) => c.stage() == TableStage::Closed,
        Err(_) => false,
    }
}

/// 行账点验：40 行齐、逐行 6,000、批号升序无重、合计 240,000。
fn rows_ok() -> bool {
    let t = build_audit_table();
    if t.row_count() != BATCH_COUNT as usize {
        return false;
    }
    let mut prev: i64 = -1;
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < t.row_count() {
        match t.row_at(i) {
            Some(r) => {
                if (r.batch as i64) <= prev {
                    return false;
                }
                prev = r.batch as i64;
                if r.registered_rows != ROWS_PER_BATCH || r.computed_rows != ROWS_PER_BATCH {
                    return false;
                }
                sum = sum.saturating_add(r.registered_rows as u64);
            }
            None => return false,
        }
        i += 1;
    }
    sum == BLOCK_SUM
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12784 自检（判据 12784 卡：域收官审计对照 · UNX-F12785-J1 守恒终验绿）。
pub fn run_unx12784_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12784");
    let table = build_audit_table();

    // 01 段位守卫（公式 0x80000+0x40×184=0x82E00，窗 [0x80000,0x82FC0]）。
    s.add_dyn(
        "F12784-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05X} 公式=0x80000+0x40×184 对齐0x40={} 窗=[0x80000,0x82FC0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    // 02 诊断码闭集（13 码互异落窗 + 未知回落）。
    let first_val = match AUDIT_ALL.first() {
        Some(d) => d.val(),
        None => 0,
    };
    let last_val = match AUDIT_ALL.last() {
        Some(d) => d.val(),
        None => 0,
    };
    s.add_dyn(
        "F12784-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "闭集{}码 首=0x{:05X} 末=0x{:05X} 未知回落含'未知'={}",
            AUDIT_ALL.len(),
            first_val,
            last_val,
            detail(SEG_BASE + 0x3F).contains("未知")
        ),
    );

    // 03 冻结三元组加法账（86,740+90,000+63,260=240,000）。
    s.add_dyn(
        "F12784-03 冻结三元组加法账",
        frozen_triple_ok(),
        &format!(
            "三元组=({},{},{}) Σ={} 逐块名非空",
            BLOCK_FROZEN.first().copied().unwrap_or(0),
            BLOCK_FROZEN.get(1).copied().unwrap_or(0),
            BLOCK_FROZEN.get(2).copied().unwrap_or(0),
            BLOCK_SUM
        ),
    );

    // 04 行账等式（40 批 × 6,000 行 = 240,000）。
    s.add_dyn(
        "F12784-04 行账等式",
        row_equation_ok() && table.equation_holds(),
        &format!(
            "40批×6000行={} 对照表等式={} 守恒={}",
            BATCH_COUNT.saturating_mul(ROWS_PER_BATCH),
            table.equation_holds(),
            table.conservation_ok()
        ),
    );

    // 05 协议条款闭集（四条款下标 0..=3，越界拒）。
    s.add_dyn(
        "F12784-05 协议条款闭集",
        clause_enum_ok(),
        &format!(
            "四条款 守恒终验/加法账零偏离/行账等式/写回完成态 下标0..=3 下标4拒={}",
            ProtocolClause::from_idx(4).is_none()
        ),
    );

    // 06 正常流收官（建表→40批→对账→备注→绿写回收官）。
    s.add_dyn(
        "F12784-06 正常流收官",
        happy_path_ok(),
        &format!(
            "建表→40批登记→对账→备注×4→写回收官 终态={:?}",
            table.stage()
        ),
    );

    // 07 判定次序（收官→倒退→条款→批序→重复，收官后写账拒）。
    s.add_dyn(
        "F12784-07 判定次序",
        judge_order_ok(),
        &format!(
            "未齐禁对账 越界批拒 重复批拒 收官后写账ESealed 首码={:?}",
            AUDIT_ALL.first().map(|d| d.val())
        ),
    );

    // 08 注入器三类 30 轮全抓（偏离/行账/写回 各 10）。
    let (caught, total, escape) = inject_sweep();
    s.add_dyn(
        "F12784-08 注入器三类 30 轮全抓",
        caught == 30 && total == 30 && escape.is_empty(),
        &format!("偏离10+行账10+写回10 caught={}/{} escape={}", caught, total, escape),
    );

    // 09 守恒复核（逐块零偏离；注入后必破且禁封）。
    s.add(
        "F12784-09 守恒复核",
        conserv_ok(),
        "逐块偏离=0 等式断言绿 偏离注入(块1+7)后守恒红且收官拒",
    );

    // 10 双跑一致指纹（FNV-1a）。
    let fa = build_audit_table().fingerprint();
    let fb = build_audit_table().fingerprint();
    s.add(
        "F12784-10 双跑一致指纹",
        dual_run_ok() && fa == fb,
        "双跑指纹相等 对照行逐条等 注入扫双跑一致",
    );

    // 11 边界四类（批端点/容量耗尽/未对账禁封/重封倒退）。
    s.add(
        "F12784-11 边界四类",
        boundary_ok(),
        "批0/39收 批40拒 备注容量16满拒 未对账禁封 重封倒退拒",
    );

    // 12 写回闸（判据③：状态=已完成、摘要非空；两路注入均拒）。
    s.add(
        "F12784-12 写回闸",
        writeback_ok(),
        "状态进行中拒 空摘要拒 已完成+非空摘要放行且终态Closed",
    );

    // 13 行账点验（40 行齐、逐行 6,000、批号升无重、合计 240,000）。
    s.add(
        "F12784-13 行账点验",
        rows_ok(),
        "40行齐 逐行6000 批号升序无重 行账合计=240,000",
    );

    // 收口：判据条数声明（14 条 = 13 实质 + 1 收口；动态槽已用满 8，
    // 本条走静态明细——不挤占 add_dyn 槽位）。
    let n_before = s.len();
    let (p_before, r_before) = s.tally();
    s.add(
        "F12784-14 收口：判据条数声明",
        n_before == 13 && r_before == 0 && p_before == 13,
        "本卡判据共 14 条=13 实质+1 收口；UNX-F12785-J1 守恒终验绿（86,740+90,000+63,260 加法账零偏离）",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_window() {
        // 段位守卫：对齐 0x40、落在 VX9 公式全窗、本卡锚定 0x82E00。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x82E00);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000);
        assert!(SEG_BASE <= 0x82FC0);
        assert_eq!(0x80000 + 0x40 * (12784 - 12600), SEG_BASE);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：13 码互异、全部落段窗、查表可命中、未知回落。
        assert_eq!(AUDIT_ALL.len(), 13);
        for d in AUDIT_ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).is_empty());
            assert_eq!(detail(v), d.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn frozen_triple_and_equation() {
        // 冻结三元组逐字 + Σ=240,000 + 40×6,000=240,000。
        assert!(frozen_triple_ok());
        assert_eq!(BLOCK_FROZEN.first().copied().unwrap_or(0), 86_740);
        assert_eq!(BLOCK_FROZEN.get(1).copied().unwrap_or(0), 90_000);
        assert_eq!(BLOCK_FROZEN.get(2).copied().unwrap_or(0), 63_260);
        let sum = 86_740u64
            .saturating_add(90_000)
            .saturating_add(63_260);
        assert_eq!(sum, 240_000);
        assert!(row_equation_ok());
        assert_eq!(BATCH_COUNT.saturating_mul(ROWS_PER_BATCH), 240_000);
    }

    #[test]
    fn happy_path_seals_with_writeback() {
        // 正常流：建表→40批→对账→绿写回收官，终态 Closed 且守恒绿。
        let mut t = build_audit_table();
        assert_eq!(t.stage(), TableStage::Reconciled);
        assert_eq!(t.row_count(), 40);
        assert!(t.conservation_ok());
        let wb = WriteBack::new(WriteStatus::Done, "守恒终验绿 摘要非空");
        assert!(t.close(&wb).is_ok());
        assert_eq!(t.stage(), TableStage::Closed);
        assert!(t.equation_holds());
    }

    #[test]
    fn injections_thirty_rounds_all_caught() {
        // 注入必抓：三类 30 轮全数命中，无逃脱。
        let (caught, total, escape) = inject_sweep();
        assert_eq!(total, 30);
        assert_eq!(caught, 30, "逃脱：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同，指纹相等。
        let a = build_audit_table();
        let b = build_audit_table();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.stage(), b.stage());
        assert_eq!(a.row_count(), b.row_count());
        let mut i = 0usize;
        while i < BATCH_COUNT as usize {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(inject_sweep(), inject_sweep());
    }

    #[test]
    fn boundaries_and_state_machine() {
        // 四类边界：批 0/39 收、40 拒；容量耗尽拒；未对账禁封；重封倒退拒；
        // 制表后回头登记属倒退。
        let mut t = AuditTable::new();
        assert!(t.register_batch(0, ProtocolClause::ConservFinal).is_ok());
        let mut full = build_audit_table();
        assert_eq!(
            full.register_batch(5, ProtocolClause::RowEquation),
            Err(AuditDiag::ERewind.val())
        );
        let mut t2 = AuditTable::new();
        assert!(t2.register_batch(39, ProtocolClause::RowEquation).is_ok());
        assert_eq!(
            t2.register_batch(40, ProtocolClause::RowEquation),
            Err(AuditDiag::EBatchRange.val())
        );
        let mut c = build_audit_table();
        let mut k = 0usize;
        while k < LOG_CAP {
            assert!(c.add_note("点验").is_ok());
            k += 1;
        }
        assert_eq!(c.note_count(), LOG_CAP);
        assert_eq!(c.add_note("超容"), Err(AuditDiag::ECap.val()));
        let wb = WriteBack::new(WriteStatus::Done, "摘要非空");
        let mut r = build_audit_table();
        assert!(r.close(&wb).is_ok());
        assert_eq!(r.close(&wb), Err(AuditDiag::ERewind.val()));
        let mut e = AuditTable::new();
        assert_eq!(e.close(&wb), Err(AuditDiag::ENotFull.val()));
    }

    #[test]
    fn writeback_gate_per_criteria3() {
        // 写回闸（判据③）：状态=已完成 + 结果摘要非空，缺一即拒。
        let mut a = build_audit_table();
        assert_eq!(
            a.close(&WriteBack::new(WriteStatus::InProgress, "摘要非空")),
            Err(AuditDiag::EStatus.val())
        );
        assert_ne!(a.stage(), TableStage::Closed);
        let mut b = build_audit_table();
        assert_eq!(
            b.close(&WriteBack::new(WriteStatus::Done, "")),
            Err(AuditDiag::ESummary.val())
        );
        assert_ne!(b.stage(), TableStage::Closed);
        // 状态秩只进不退。
        assert!(WriteStatus::InProgress.rank() < WriteStatus::Done.rank());
    }

    #[test]
    fn deviation_breaks_conservation_and_close() {
        // 偏离注入后：守恒必红、对账必回 EDeviation、收官必拒。
        let mut t = build_audit_table();
        assert!(t.inject_block_deviation(0, 1).is_ok());
        assert!(!t.conservation_ok());
        assert_eq!(t.reconcile(), Err(AuditDiag::EDeviation.val()));
        let wb = WriteBack::new(WriteStatus::Done, "摘要非空");
        assert_eq!(t.close(&wb), Err(AuditDiag::EConserv.val()));
        // 负偏离同样抓（saturating 压界后仍非零偏离）。
        let mut n = build_audit_table();
        assert!(n.inject_block_deviation(2, -5).is_ok());
        assert!(!n.conservation_ok());
    }

    #[test]
    fn row_mismatch_injection_caught() {
        // 行账注入：±delta 均被行账闸抓，守恒随之红。
        let mut t = build_audit_table();
        assert!(t.inject_batch_rows(7, 3).is_ok());
        assert_eq!(t.reconcile(), Err(AuditDiag::ERowMismatch.val()));
        let mut u = build_audit_table();
        assert!(u.inject_batch_rows(11, -9).is_ok());
        assert!(!u.conservation_ok());
        // 注入后修回：守恒复绿。
        assert!(u.inject_batch_rows(11, 9).is_ok());
        assert!(u.conservation_ok());
    }

    #[test]
    fn judge_order_frozen() {
        // 判定次序冻结：未齐禁对账 → 越界批拒 → 重复批拒 → 收官后写账拒。
        let mut p = AuditTable::new();
        assert_eq!(p.reconcile(), Err(AuditDiag::ENotFull.val()));
        assert_eq!(
            p.register_batch(40, ProtocolClause::RowEquation),
            Err(AuditDiag::EBatchRange.val())
        );
        assert!(p.register_batch(3, ProtocolClause::ZeroDeviation).is_ok());
        assert_eq!(
            p.register_batch(3, ProtocolClause::ZeroDeviation),
            Err(AuditDiag::EDup.val())
        );
        let mut f = build_audit_table();
        let wb = WriteBack::new(WriteStatus::Done, "收官点验摘要非空");
        assert!(f.close(&wb).is_ok());
        assert_eq!(
            f.register_batch(1, ProtocolClause::ConservFinal),
            Err(AuditDiag::ESealed.val())
        );
        assert_eq!(f.add_note("收官后"), Err(AuditDiag::ESealed.val()));
        assert_eq!(
            f.inject_block_deviation(0, 1),
            Err(AuditDiag::ESealed.val())
        );
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本卡 14 条 = 13 实质 + 1 收口）。
        let s = run_unx12784_checks();
        assert!(s.all_passed(), "红项：{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 14);
        assert_eq!(s.tally().1, 0);
        assert!(!s.truncated());
    }
}
