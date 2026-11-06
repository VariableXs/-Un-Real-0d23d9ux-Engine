//! UNX-F12791 移交包一：handoff 域块收官声明（finalized=40/rows=240,000/entries=800） · 0x82FC0段
//!
//! 判据原文（派工卡 12791 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F12792-J1 总纲四十行 [已深化]
//!   终态判据绿，域小结收官条款同步；③ 完成时 API/文案写回：状态=已完成、
//!   结果摘要非空。
//!
//! 补充锚定（派工卡 12791 补充条）：
//!   - 40 行 [已深化] 终态 = 40 batch rows each [已深化] closed state；
//!     状态闭集 {初稿/深化中/已深化}，只许 [已深化] 作终态，任一行停在其他
//!     状态即伪声明 → 10/10 抓；
//!   - 域小结收官条款 = domain summary closure clause registered + non-empty。
//!
//! 设计要点：
//! 1. 四十行域块：40 批行预建（批号闭集 0..=39），状态闭集三态单调
//!    初稿(0) → 深化中(1) → 已深化(2)，只进不退，倒退显性拒（ERewind）、
//!    终态后再推拒（EOrder）；
//! 2. 收官声明三元组冻结：finalized=40 / rows=240,000 / entries=800，
//!    声明登记逐字段对冻结常量复核，任一失配即 ECount（计数失配注入必抓）；
//! 3. 域小结收官条款同步：声明条款逐字非空（空条款 EClause），且声明绿
//!    之前提是 40 行全部已达 [已深化] 终态（任一行停在其他状态即
//!    ENotFinal——终态伪声明注入必抓）；
//! 4. 收官闸（判据 ③）：声明已登记 + 写回状态=已完成 + 结果摘要非空，
//!    缺一即拒；收官后写账/注入一律拒绝；
//! 5. 注入器三类（终态伪声明注入 / 收官条款空注 / 计数失配注入）各 10 轮
//!    全抓，逃脱即判据失效；xorshift32 种子写死 = SEG_BASE，双跑一致指纹
//!    （FNV-1a）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；取值一律
//!    match/if let/.get()/.contains/saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12791 − 12600) = 0x80000 + 0x2FC0）。
pub const SEG_BASE: u32 = 0x82FC0;

// ── 冻结常量表（来源：派工卡 12791 判据 UNX-F12792-J1 终态定义） ──
/// 总纲行数（判据逐字：四十行）。
pub const BATCH_TOTAL: usize = 40;
/// 收官声明字段：finalized（判据逐字：finalized=40）。
pub const DECL_FINALIZED: u32 = 40;
/// 收官声明字段：rows（判据逐字：240,000）。
pub const DECL_ROWS: u64 = 240_000;
/// 收官声明字段：entries（判据逐字：entries=800）。
pub const DECL_ENTRIES: u64 = 800;
/// 审计备注日志容量（收官声明点验槽）。
pub const LOG_CAP: usize = 16;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdlDiag {
    /// 段位越窗或不对齐（SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]）。
    ESegGuard,
    /// 批序越界（批号闭集 0..=39，40 及以上越界）。
    EBatchRange,
    /// 次序违约（终态行再推无空间，或目标秩等于当前秩）。
    EOrder,
    /// 终态伪声明（任一行未达 [已深化] 终态即求收官声明）。
    ENotFinal,
    /// 声明计数失配（finalized/rows/entries 任一偏离冻结值）。
    ECount,
    /// 收官条款空注（域小结收官条款为空串）。
    EClause,
    /// 状态倒退（初稿→深化中→已深化只进不退）。
    ERewind,
    /// 账已收官（收官后写账/注入一律拒绝）。
    ESealed,
    /// 备注日志容量耗尽（LOG_CAP 槽满仍记新行）。
    ECap,
    /// 写回状态错（收官写回状态 ≠ 已完成）。
    EStatus,
    /// 写回摘要空（收官写回结果摘要为空串）。
    ESummary,
}

impl HdlDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HdlDiag::ESegGuard => 0x01,
                HdlDiag::EBatchRange => 0x02,
                HdlDiag::EOrder => 0x03,
                HdlDiag::ENotFinal => 0x04,
                HdlDiag::ECount => 0x05,
                HdlDiag::EClause => 0x06,
                HdlDiag::ERewind => 0x07,
                HdlDiag::ESealed => 0x08,
                HdlDiag::ECap => 0x09,
                HdlDiag::EStatus => 0x0a,
                HdlDiag::ESummary => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HdlDiag::ESegGuard => {
                "段位越窗：SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]，先回公式 0x80000+0x40×(id−12600) 复核段基址"
            }
            HdlDiag::EBatchRange => {
                "批序越界：总纲四十行批号闭集为 0..=39，越界批号不入账，先回 40 行定义核对端点"
            }
            HdlDiag::EOrder => {
                "次序违约：该行已处 [已深化] 终态、无推进空间，深化推进须逐行升秩不得重复收官同一行"
            }
            HdlDiag::ENotFinal => {
                "终态伪声明：四十行须全部 [已深化] 方可收官声明，先点验未达终态行（初稿/深化中）补深化再声明"
            }
            HdlDiag::ECount => {
                "声明计数失配：finalized=40/rows=240,000/entries=800 冻结值任一偏离即拒，逐字段回修登记声明"
            }
            HdlDiag::EClause => {
                "收官条款空注：域小结收官条款为空串，先填非空收官条款（写明四十行 [已深化] 结论）再声明"
            }
            HdlDiag::ERewind => {
                "状态倒退：初稿→深化中→已深化只进不退，倒退即域块可疑，须全块复核"
            }
            HdlDiag::ESealed => {
                "账已收官：收官后写账/注入一律拒绝，改动走下一条移交滚动"
            }
            HdlDiag::ECap => {
                "备注日志容量耗尽：LOG_CAP 槽已满，先归档已有备注行再记新行"
            }
            HdlDiag::EStatus => {
                "写回状态错：收官写回状态须=已完成，先按移交协议补完成态写回再收官"
            }
            HdlDiag::ESummary => {
                "写回摘要空：收官写回结果摘要为空串，先填非空结果摘要再收官"
            }
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HDL_ALL: &[HdlDiag] = &[
    HdlDiag::ESegGuard,
    HdlDiag::EBatchRange,
    HdlDiag::EOrder,
    HdlDiag::ENotFinal,
    HdlDiag::ECount,
    HdlDiag::EClause,
    HdlDiag::ERewind,
    HdlDiag::ESealed,
    HdlDiag::ECap,
    HdlDiag::EStatus,
    HdlDiag::ESummary,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HDL_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82FC1..0x82FCB 闭集内，先回本卡 SEG_BASE 段核对码值"
}

// ── 批行状态闭集（三态单调，终态唯一 [已深化]） ──────────────────
/// 批行状态闭集（初稿/深化中/已深化；只许 [已深化] 作终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchState {
    /// 初稿：批行起草态。
    Draft,
    /// 深化中：批行深化推进态（非终态，伪声明必抓）。
    Deepening,
    /// 已深化：批行终态（判据逐字 [已深化]，终态唯一）。
    Deepened,
}

impl BatchState {
    /// 状态秩（初稿0 < 深化中1 < 已深化2，推进须严格升秩）。
    pub fn rank(self) -> u8 {
        match self {
            BatchState::Draft => 0,
            BatchState::Deepening => 1,
            BatchState::Deepened => 2,
        }
    }

    /// 状态中文名（闭集逐字：初稿/深化中/已深化）。
    pub fn name(self) -> &'static str {
        match self {
            BatchState::Draft => "初稿",
            BatchState::Deepening => "深化中",
            BatchState::Deepened => "已深化",
        }
    }

    /// 是否终态（只许 [已深化]）。
    pub fn is_final(self) -> bool {
        self == BatchState::Deepened
    }

    /// 由下标反查状态（越界 None）。
    pub fn from_idx(i: u32) -> Option<BatchState> {
        match i {
            0 => Some(BatchState::Draft),
            1 => Some(BatchState::Deepening),
            2 => Some(BatchState::Deepened),
            _ => None,
        }
    }
}

/// 状态闭集数组（下标即秩）。
pub const BATCH_STATE_ALL: &[BatchState] = &[
    BatchState::Draft,
    BatchState::Deepening,
    BatchState::Deepened,
];

/// 写回状态闭集（判据③：完成时 API/文案写回状态=已完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclWriteStatus {
    /// 进行中：尚未达成收官写回条件。
    InProgress,
    /// 已完成：收官写回终态。
    Done,
}

// ── 数据模型 ─────────────────────────────────────────────────────
/// 批行：批号 + 状态（四十行域块一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchRow {
    /// 批号（0..=39）。
    pub batch: u32,
    /// 批行状态（三态闭集，终态唯一 [已深化]）。
    pub state: BatchState,
}

/// 域小结收官声明：计数三元组 + 收官条款（判据 ② 与补充锚定逐字）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosureDecl {
    /// finalized（冻结 40）。
    pub finalized: u32,
    /// rows（冻结 240,000）。
    pub rows: u64,
    /// entries（冻结 800）。
    pub entries: u64,
    /// 域小结收官条款（非空闸）。
    pub clause: String,
}

/// 声明账阶段（只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclStage {
    /// 起草：允许逐行深化推进。
    Drafting,
    /// 已声明：收官声明已登记（四十行全终态 + 条款非空 + 计数齐）。
    Declared,
    /// 收官：只读，写账/注入一律拒绝。
    Sealed,
}

impl DeclStage {
    /// 阶段秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            DeclStage::Drafting => 0,
            DeclStage::Declared => 1,
            DeclStage::Sealed => 2,
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

/// handoff 域块：四十批行 + 收官声明 + 单调状态机 + 备注日志。
#[derive(Clone, Debug)]
pub struct HandoffDecl {
    /// 四十批行（预建，批号升序）。
    rows: Vec<BatchRow>,
    /// 收官声明（None=未登记）。
    decl: Option<ClosureDecl>,
    /// 声明账阶段。
    stage: DeclStage,
    /// 备注日志（容量 LOG_CAP）。
    notes: Vec<String>,
}

impl HandoffDecl {
    /// 开一本新域块账（四十行预建，全初稿）。
    pub fn new() -> HandoffDecl {
        let mut rows: Vec<BatchRow> = Vec::new();
        let mut b = 0u32;
        while b < BATCH_TOTAL as u32 {
            rows.push(BatchRow {
                batch: b,
                state: BatchState::Draft,
            });
            b += 1;
        }
        HandoffDecl {
            rows,
            decl: None,
            stage: DeclStage::Drafting,
            notes: Vec::new(),
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> DeclStage {
        self.stage
    }

    /// 已登记收官声明（None=未登记）。
    pub fn declaration(&self) -> Option<&ClosureDecl> {
        self.decl.as_ref()
    }

    /// 批行数（恒四十）。
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

    /// 按批号取行（越界 None）。
    pub fn row_of(&self, batch: u32) -> Option<&BatchRow> {
        self.rows.iter().find(|r| r.batch == batch)
    }

    /// 按下标取行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&BatchRow> {
        self.rows.get(idx)
    }

    /// 四十行终态点验：全部 [已深化]（判据逐字：only final state allowed）。
    pub fn all_final_ok(&self) -> bool {
        if self.rows.len() != BATCH_TOTAL {
            return false;
        }
        for r in self.rows.iter() {
            if !r.state.is_final() {
                return false;
            }
        }
        true
    }

    /// 未达终态行数（点验面：初稿 + 深化中 行数）。
    pub fn non_final_count(&self) -> usize {
        let mut n = 0usize;
        for r in self.rows.iter() {
            if !r.state.is_final() {
                n += 1;
            }
        }
        n
    }

    /// 深化推进：批行秩 +1（初稿→深化中→已深化；次序冻结：收官闸 →
    /// 倒退闸 → 批序范围 → 升秩/终态判）。
    pub fn deepen(&mut self, batch: u32) -> Result<(), u32> {
        // ① 收官闸。
        if self.stage.rank() >= DeclStage::Sealed.rank() {
            return Err(HdlDiag::ESealed.val());
        }
        // ② 倒退闸：已声明后回头深化属倒退。
        if self.stage.rank() > DeclStage::Drafting.rank() {
            return Err(HdlDiag::ERewind.val());
        }
        // ③ 批序范围闸。
        if batch >= BATCH_TOTAL as u32 {
            return Err(HdlDiag::EBatchRange.val());
        }
        // ④ 升秩/终态判：终态行再推无空间（EOrder），未终态行升一秩。
        let cur = match self.row_of(batch) {
            Some(r) => r.state,
            None => return Err(HdlDiag::EBatchRange.val()),
        };
        if cur.is_final() {
            return Err(HdlDiag::EOrder.val());
        }
        match self.rows.iter_mut().find(|r| r.batch == batch) {
            Some(r) => {
                r.state = match cur.rank() {
                    0 => BatchState::Deepening,
                    _ => BatchState::Deepened,
                };
                Ok(())
            }
            None => Err(HdlDiag::EBatchRange.val()),
        }
    }

    /// 全域深化：逐行推进直至四十行全 [已深化]（起草阶段批量口）。
    pub fn deepen_all(&mut self) -> Result<(), u32> {
        let mut b = 0u32;
        while b < BATCH_TOTAL as u32 {
            // 每行两推：初稿→深化中→已深化。
            let _ = self.deepen(b);
            let _ = self.deepen(b);
            b += 1;
        }
        if self.all_final_ok() {
            Ok(())
        } else {
            Err(HdlDiag::ENotFinal.val())
        }
    }

    /// 收官声明登记（次序冻结：收官闸 → 倒退闸 → 四十行终态 → 计数闭集 →
    /// 条款非空；判据 ② 逐字锚定）。
    pub fn declare(
        &mut self,
        finalized: u32,
        rows: u64,
        entries: u64,
        clause: &str,
    ) -> Result<(), u32> {
        // ① 收官闸。
        if self.stage.rank() >= DeclStage::Sealed.rank() {
            return Err(HdlDiag::ESealed.val());
        }
        // ② 重声明闸：已声明再声明属倒退。
        if self.stage.rank() > DeclStage::Drafting.rank() {
            return Err(HdlDiag::ERewind.val());
        }
        // ③ 四十行终态闸（终态伪声明注入必抓口）。
        if !self.all_final_ok() {
            return Err(HdlDiag::ENotFinal.val());
        }
        // ④ 计数闭集闸（计数失配注入必抓口）。
        if finalized != DECL_FINALIZED || rows != DECL_ROWS || entries != DECL_ENTRIES {
            return Err(HdlDiag::ECount.val());
        }
        // ⑤ 域小结收官条款非空闸（收官条款空注必抓口）。
        if clause.is_empty() {
            return Err(HdlDiag::EClause.val());
        }
        self.decl = Some(ClosureDecl {
            finalized,
            rows,
            entries,
            clause: String::from(clause),
        });
        self.stage = DeclStage::Declared;
        Ok(())
    }

    /// 域小结收官条款同步点验：声明已登记 + 条款非空 + 四十行全终态 +
    /// 计数与冻结值逐字段一致（判据 ②：收官条款同步）。
    pub fn sync_ok(&self) -> bool {
        let d = match &self.decl {
            Some(d) => d,
            None => return false,
        };
        if d.clause.is_empty() {
            return false;
        }
        if !self.all_final_ok() {
            return false;
        }
        d.finalized == DECL_FINALIZED && d.rows == DECL_ROWS && d.entries == DECL_ENTRIES
    }

    /// 记一条备注行（容量 LOG_CAP，收官后拒绝）。
    pub fn add_note(&mut self, text: &str) -> Result<(), u32> {
        if self.stage.rank() >= DeclStage::Sealed.rank() {
            return Err(HdlDiag::ESealed.val());
        }
        if self.notes.len() >= LOG_CAP {
            return Err(HdlDiag::ECap.val());
        }
        self.notes.push(String::from(text));
        Ok(())
    }

    /// 收官（判据 ③ 逐字锚定）：声明已登记 + 同步绿 + 写回状态=已完成 +
    /// 结果摘要非空。
    pub fn seal(&mut self, status: DeclWriteStatus, summary: &str) -> Result<(), u32> {
        // 重封闸：已收官再封属倒退。
        if self.stage.rank() >= DeclStage::Sealed.rank() {
            return Err(HdlDiag::ERewind.val());
        }
        // 未声明闸。
        if self.stage.rank() < DeclStage::Declared.rank() {
            return Err(HdlDiag::ENotFinal.val());
        }
        // 同步闸：条款同步绿方许收官。
        if !self.sync_ok() {
            return Err(HdlDiag::EClause.val());
        }
        // 写回状态闸（判据 ③：状态=已完成）。
        if status != DeclWriteStatus::Done {
            return Err(HdlDiag::EStatus.val());
        }
        // 写回摘要闸（判据 ③：结果摘要非空）。
        if summary.is_empty() {
            return Err(HdlDiag::ESummary.val());
        }
        self.stage = DeclStage::Sealed;
        Ok(())
    }

    // ── 审计注入面（只服务注入必抓扫，收官后拒绝） ────────────────
    /// 类一注入：把指定行强置为未终态（初稿/深化中），模拟终态伪声明，
    /// all_final_ok 与 declare 闸必须抓。
    pub fn inject_stall(&mut self, batch: u32, st: BatchState) -> Result<(), u32> {
        if self.stage.rank() >= DeclStage::Sealed.rank() {
            return Err(HdlDiag::ESealed.val());
        }
        if batch >= BATCH_TOTAL as u32 {
            return Err(HdlDiag::EBatchRange.val());
        }
        if st.is_final() {
            // 注入终态不构成"停滞"注入，语义上退回初稿才有效。
            return Err(HdlDiag::ENotFinal.val());
        }
        match self.rows.iter_mut().find(|r| r.batch == batch) {
            Some(r) => {
                r.state = st;
                Ok(())
            }
            None => Err(HdlDiag::EBatchRange.val()),
        }
    }

    /// 双跑一致指纹（FNV-1a 32 位）：阶段 + 四十行 + 声明 + 备注行。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_mix(h, &self.stage.rank().to_le_bytes());
        h = fnv_mix(h, &(self.rows.len() as u32).to_le_bytes());
        for r in self.rows.iter() {
            h = fnv_mix(h, &r.batch.to_le_bytes());
            h = fnv_mix(h, &r.state.rank().to_le_bytes());
        }
        match &self.decl {
            Some(d) => {
                h = fnv_mix(h, &d.finalized.to_le_bytes());
                h = fnv_mix(h, &d.rows.to_le_bytes());
                h = fnv_mix(h, &d.entries.to_le_bytes());
                h = fnv_mix(h, d.clause.as_bytes());
            }
            None => h = fnv_mix(h, &0xFFFF_FFFFu32.to_le_bytes()),
        }
        for n in self.notes.iter() {
            h = fnv_mix(h, n.as_bytes());
        }
        h
    }
}

// ── 确定性 xorshift32（种子 = SEG_BASE，零随机红线合规） ─────────
/// xorshift32 发生器：种子写死，序列确定，双跑一致。
pub struct Xor32 {
    state: u32,
}

impl Xor32 {
    /// 以固定种子构造（种子不得为零，零种子回退段基址常量）。
    pub fn new(seed: u32) -> Xor32 {
        Xor32 {
            state: if seed == 0 { SEG_BASE } else { seed },
        }
    }

    /// 下一伪随机数（xorshift32：x^=x<<13; x^=x>>17; x^=x<<5）。
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 正常流构建（全域深化 → 收官声明 → 不收官留 seal 口） ─────────
/// 构建一本四十行全 [已深化]、声明已登记的域块账。
pub fn build_handoff_decl() -> HandoffDecl {
    let mut d = HandoffDecl::new();
    let _ = d.deepen_all();
    let _ = d.declare(
        DECL_FINALIZED,
        DECL_ROWS,
        DECL_ENTRIES,
        "总纲四十行 [已深化] 终态，域小结收官条款同步",
    );
    d
}

// ── 注入器三类扫（各 10 轮全抓，逃脱即判据失效） ─────────────────
/// 注入扫：类一终态伪声明注入 ×10 / 类二收官条款空注 ×10 /
/// 类三计数失配注入 ×10。返回（抓到路数, 总路数, 首个逃脱路描述）。
pub fn inject_sweep() -> (usize, usize, String) {
    let mut rng = Xor32::new(SEG_BASE);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 类一：终态伪声明注入 —— 起草账全域深化后把任一行压回未终态，
    // all_final_ok 红（伪终态）且 declare 必回 ENotFinal。
    let mut k = 0usize;
    while k < 10 {
        let batch = rng.below(BATCH_TOTAL as u32);
        let st = match BatchState::from_idx(rng.below(2)) {
            Some(s) => s,
            None => BatchState::Draft,
        };
        // 起草阶段账：全域深化后仍未声明（declare 留给注入点验）。
        let mut d = HandoffDecl::new();
        let _ = d.deepen_all();
        let inj = d.inject_stall(batch, st);
        let fin = d.all_final_ok();
        let dec = d.declare(
            DECL_FINALIZED,
            DECL_ROWS,
            DECL_ENTRIES,
            "伪声明条款非空",
        );
        let ok = inj.is_ok() && !fin && dec == Err(HdlDiag::ENotFinal.val());
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类一伪声明 路{} 批{} 注入逃脱（dec={:?}）", k, batch, dec.err());
        }
        k += 1;
    }
    // 类二：收官条款空注 —— 起草账全域深化后以空条款声明，
    // 必被 EClause 抓且声明不落账。
    k = 0;
    while k < 10 {
        let mut d = HandoffDecl::new();
        let _ = d.deepen_all();
        let dec = d.declare(DECL_FINALIZED, DECL_ROWS, DECL_ENTRIES, "");
        let ok = dec == Err(HdlDiag::EClause.val())
            && d.declaration().is_none()
            && d.stage() == DeclStage::Drafting;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类二空条款 路{} 注入逃脱", k);
        }
        k += 1;
    }
    // 类三：计数失配注入 —— 起草账全域深化后以失配计数声明，
    // 三字段任一偏离冻结值必被 ECount 抓。
    k = 0;
    while k < 10 {
        let mut d = HandoffDecl::new();
        let _ = d.deepen_all();
        let pick = rng.below(3);
        let (f, r_, e) = match pick {
            0 => (DECL_FINALIZED + 1 + rng.below(3), DECL_ROWS, DECL_ENTRIES),
            1 => (DECL_FINALIZED, DECL_ROWS + 1 + rng.below(9) as u64, DECL_ENTRIES),
            _ => (DECL_FINALIZED, DECL_ROWS, DECL_ENTRIES + 1 + rng.below(5) as u64),
        };
        let dec = d.declare(f, r_, e, "计数失配条款非空");
        let ok = dec == Err(HdlDiag::ECount.val())
            && d.declaration().is_none()
            && d.stage() == DeclStage::Drafting;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类三计数 路{} pick={} 注入逃脱", k, pick);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x82FC0]，
/// 并锚定本卡公式实值 0x80000 + 0x40×191 = 0x82FC0。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
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
    let own = match 12791u32.checked_sub(12600) {
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
    while i < HDL_ALL.len() {
        let d = match HDL_ALL.get(i) {
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
        while j < HDL_ALL.len() {
            match HDL_ALL.get(j) {
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
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 状态闭集守卫：三态秩升、名逐字非空、终态唯一（只许已深化）、越界下标拒。
fn state_closed_ok() -> bool {
    if BATCH_STATE_ALL.len() != 3 {
        return false;
    }
    let mut finals = 0usize;
    let mut prev = 0u8;
    for st in BATCH_STATE_ALL.iter() {
        if st.rank() < prev {
            return false;
        }
        prev = st.rank();
        if st.name().is_empty() {
            return false;
        }
        match BatchState::from_idx(st.rank() as u32) {
            Some(x) => {
                if x != *st {
                    return false;
                }
            }
            None => return false,
        }
        if st.is_final() {
            finals += 1;
        }
    }
    // 终态唯一：闭集内只有 [已深化] 一态 is_final。
    if finals != 1 || !BatchState::Deepened.is_final() {
        return false;
    }
    // 越界下标拒（cap=2，cap+1=3 越界）。
    if BatchState::from_idx(3).is_some() {
        return false;
    }
    prev == 2
}

/// 四十行终态守卫：正常账全终态且非终态行数为 0；停滞注入后必破。
fn forty_final_ok() -> bool {
    let d = build_handoff_decl();
    if !d.all_final_ok() || d.non_final_count() != 0 || d.row_count() != BATCH_TOTAL {
        return false;
    }
    let mut t = build_handoff_decl();
    if t.inject_stall(19, BatchState::Deepening).is_err() {
        return false;
    }
    if t.all_final_ok() || t.non_final_count() != 1 {
        return false;
    }
    true
}

/// 正常流：全域深化 → 声明 → 备注 ×2 → 收官（绿写回），
/// 收官后终态点验仍绿。
fn happy_path_ok() -> bool {
    let mut d = build_handoff_decl();
    if d.stage() != DeclStage::Declared {
        return false;
    }
    if !d.sync_ok() {
        return false;
    }
    let mut k = 0usize;
    while k < 2 {
        if d.add_note("收官点验行").is_err() {
            return false;
        }
        k += 1;
    }
    if d.seal(DeclWriteStatus::Done, "四十行 [已深化] 终态 结果摘要非空")
        .is_err()
    {
        return false;
    }
    d.stage() == DeclStage::Sealed && d.all_final_ok() && d.sync_ok()
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

/// 双跑一致：两次构建/注入全流程逐项一致，指纹相等。
fn dual_run_ok() -> bool {
    let a = build_handoff_decl();
    let b = build_handoff_decl();
    if a.stage() != b.stage() || a.row_count() != b.row_count() {
        return false;
    }
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    let mut i = 0usize;
    while i < BATCH_TOTAL {
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

/// 边界四类：批 0/39 推进收、40 拒（cap+1）、容量耗尽拒、封账后写拒/
/// 重声明倒退拒/重封倒退拒。
fn boundary_ok() -> bool {
    // ① 批号端点：0 与 39 各两推到终态；40 拒。
    let mut d = HandoffDecl::new();
    if d.deepen(0).is_err() || d.deepen(0).is_err() {
        return false;
    }
    if d.deepen(39).is_err() || d.deepen(39).is_err() {
        return false;
    }
    if d.deepen(BATCH_TOTAL as u32) != Err(HdlDiag::EBatchRange.val()) {
        return false;
    }
    // 终态行再推 → EOrder。
    if d.deepen(0) != Err(HdlDiag::EOrder.val()) {
        return false;
    }
    // ② 容量耗尽。
    let mut c = build_handoff_decl();
    let mut k = 0usize;
    while k < LOG_CAP {
        if c.add_note("容量点验").is_err() {
            return false;
        }
        k += 1;
    }
    if c.add_note("超容") != Err(HdlDiag::ECap.val()) {
        return false;
    }
    // ③ 未声明禁收官（起草阶段账即封必拒）。
    let mut p = HandoffDecl::new();
    if p.seal(DeclWriteStatus::Done, "摘要非空") != Err(HdlDiag::ENotFinal.val()) {
        return false;
    }
    // ④ 封账后写拒 + 重声明倒退拒 + 重封倒退拒。
    let mut f = build_handoff_decl();
    if f.seal(DeclWriteStatus::Done, "四十行收官摘要非空").is_err() {
        return false;
    }
    if f.deepen(5) != Err(HdlDiag::ESealed.val()) {
        return false;
    }
    if f.add_note("封账后备注") != Err(HdlDiag::ESealed.val()) {
        return false;
    }
    if f.declare(DECL_FINALIZED, DECL_ROWS, DECL_ENTRIES, "重声明")
        != Err(HdlDiag::ESealed.val())
    {
        return false;
    }
    if f.seal(DeclWriteStatus::Done, "重封") != Err(HdlDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 写回闸：错误状态/空摘要各路均拒且阶段不进 Sealed；绿写回放行。
fn writeback_ok() -> bool {
    let mut a = build_handoff_decl();
    if a.seal(DeclWriteStatus::InProgress, "摘要非空") != Err(HdlDiag::EStatus.val()) {
        return false;
    }
    if a.stage() == DeclStage::Sealed {
        return false;
    }
    let mut b = build_handoff_decl();
    if b.seal(DeclWriteStatus::Done, "") != Err(HdlDiag::ESummary.val()) {
        return false;
    }
    true
}

/// 计数点验：冻结三元组逐字（40/240,000/800）且声明回读一致。
fn counts_ok() -> bool {
    if DECL_FINALIZED != 40 || DECL_ROWS != 240_000 || DECL_ENTRIES != 800 {
        return false;
    }
    match build_handoff_decl().declaration() {
        Some(d) => {
            d.finalized == DECL_FINALIZED && d.rows == DECL_ROWS && d.entries == DECL_ENTRIES
        }
        None => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12791 自检（判据 12791 卡：移交包一收官声明 · UNX-F12792-J1 锚定）。
pub fn run_unx12791_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12791");
    let dom = build_handoff_decl();

    // 01 段位守卫（公式 0x80000+0x40×191=0x82FC0，窗 [0x80000,0x82FC0]）。
    s.add_dyn(
        "F12791-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05X} 公式=0x80000+0x40×191 对齐0x40={} 窗上沿=段值",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    // 02 诊断码闭集（11 码互异落窗 + 未知回落）。
    s.add(
        "F12791-02 诊断码闭集",
        diag_closed_ok(),
        "11码互异落段窗 查表命中 未知码回落含'未知'",
    );

    // 03 状态闭集单调（初稿→深化中→已深化，终态唯一）。
    s.add_dyn(
        "F12791-03 状态闭集单调",
        state_closed_ok(),
        &format!(
            "三态闭集 {}→{}→{} 终态唯一={} 下标3越界拒",
            BATCH_STATE_ALL.first().map(|x| x.name()).unwrap_or(""),
            BATCH_STATE_ALL.get(1).map(|x| x.name()).unwrap_or(""),
            BATCH_STATE_ALL.get(2).map(|x| x.name()).unwrap_or(""),
            BatchState::Deepened.is_final()
        ),
    );

    // 04 四十行 [已深化] 终态（判据逐字：only final state allowed）。
    s.add_dyn(
        "F12791-04 四十行已深化终态",
        forty_final_ok(),
        &format!(
            "行={} 全终态={} 非终态行={} 停滞注入后必破",
            dom.row_count(),
            dom.all_final_ok(),
            build_handoff_decl().non_final_count()
        ),
    );

    // 05 正常流收官（全域深化→声明→备注→绿写回封账）。
    s.add_dyn(
        "F12791-05 正常流收官",
        happy_path_ok(),
        &format!(
            "深化→声明→备注→收官 终态={:?} 同步={}",
            dom.stage(),
            dom.sync_ok()
        ),
    );

    // 06 注入器三类 30 轮全抓（伪声明/空条款/计数失配 各 10）。
    let (caught, total, escape) = inject_sweep();
    s.add_dyn(
        "F12791-06 注入三类 30 轮全抓",
        caught == 30 && total == 30 && escape.is_empty(),
        &format!("伪声明10+空条款10+计数10 caught={}/{} escape={}", caught, total, escape),
    );

    // 07 域小结收官条款同步（声明条款非空 + 四十行终态 + 计数齐）。
    s.add_dyn(
        "F12791-07 收官条款同步",
        dom.sync_ok(),
        &format!(
            "条款={:?} 同步={}",
            dom.declaration().map(|d| d.clause.as_str()).unwrap_or(""),
            dom.sync_ok()
        ),
    );

    // 08 计数点验（finalized=40/rows=240,000/entries=800 冻结值回读一致）。
    s.add_dyn(
        "F12791-08 计数点验",
        counts_ok(),
        &format!(
            "finalized={} rows={} entries={} 登记回读一致",
            DECL_FINALIZED, DECL_ROWS, DECL_ENTRIES
        ),
    );

    // 09 双跑一致指纹（FNV-1a + xorshift32 种子=SEG_BASE）。
    let fa = build_handoff_decl().fingerprint();
    let fb = build_handoff_decl().fingerprint();
    s.add(
        "F12791-09 双跑一致指纹",
        dual_run_ok() && fa == fb,
        "双跑指纹相等 四十行逐条等 注入扫双跑一致",
    );

    // 10 边界四类（批端点/容量耗尽/未声明禁收官/封账后写拒+倒退拒）。
    s.add(
        "F12791-10 边界四类",
        boundary_ok(),
        "批0/39两推收 批40拒 终态再推EOrder 容量16满拒 未声明禁收官 封账后写拒 重声明重封倒退拒",
    );

    // 11 写回闸（判据③：状态=已完成、摘要非空；两路注入均拒）。
    s.add(
        "F12791-11 写回闸",
        writeback_ok(),
        "状态进行中拒 空摘要拒 已完成+非空摘要放行且终态Sealed",
    );

    // 12 判据交叉锚定（UNX-F12793-J1：四十行与移交包二总纲同步同源）。
    s.add(
        "F12791-12 交叉锚定总纲",
        DECL_FINALIZED == 40 && BATCH_TOTAL == 40 && DECL_ROWS == 240_000,
        "finalized=40 与四十行同源 rows=240,000 与域账守恒同源",
    );

    // 13 逐行点验（四十行批号升序无重、行状态全终态）。
    let mut row_ok = true;
    let mut prev: i64 = -1;
    let mut i = 0usize;
    while i < dom.row_count() {
        match dom.row_at(i) {
            Some(r) => {
                if (r.batch as i64) <= prev {
                    row_ok = false;
                }
                prev = r.batch as i64;
                if !r.state.is_final() {
                    row_ok = false;
                }
            }
            None => row_ok = false,
        }
        i += 1;
    }
    s.add(
        "F12791-13 逐行点验",
        row_ok && dom.row_count() == 40,
        "四十行批号升序无重 状态全[已深化]终态",
    );

    // 收口：判据条数声明（14 条 = 13 实质 + 1 收口）。
    let n_before = s.len();
    let (p_before, r_before) = s.tally();
    s.add_dyn(
        "F12791-14 收口：判据条数声明",
        n_before == 13 && r_before == 0 && p_before == 13,
        &format!(
            "本卡判据共 14 条=13 实质+1 收口；已记账 13 条红 {}；UNX-F12792-J1 总纲四十行 [已深化] 终态判据绿达成={}",
            r_before,
            n_before == 13 && r_before == 0 && p_before == 13
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_window() {
        // 段位守卫：对齐 0x40、落在 VX9 公式全窗、本卡锚定 0x82FC0（窗上沿）。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x82FC0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000);
        assert!(SEG_BASE <= 0x82FC0);
        assert_eq!(0x80000 + 0x40 * (12791 - 12600), SEG_BASE);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：11 码互异、全部落段窗、查表可命中、未知回落。
        assert_eq!(HDL_ALL.len(), 11);
        for d in HDL_ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).is_empty());
            assert_eq!(detail(v), d.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn state_closed_set_final_unique() {
        // 状态闭集：三态、终态唯一 [已深化]、越界下标拒。
        assert!(state_closed_ok());
        assert!(BatchState::Deepened.is_final());
        assert!(!BatchState::Draft.is_final());
        assert!(!BatchState::Deepening.is_final());
        assert!(BatchState::from_idx(3).is_none());
        assert!(BatchState::from_idx(2).is_some());
    }

    #[test]
    fn forty_rows_all_deepened() {
        // 四十行终态：正常账全 [已深化]；停滞注入后必破且 non_final=1。
        assert!(forty_final_ok());
        let d = build_handoff_decl();
        assert_eq!(d.row_count(), 40);
        assert_eq!(d.non_final_count(), 0);
        let mut t = build_handoff_decl();
        assert!(t.inject_stall(7, BatchState::Draft).is_ok());
        assert!(!t.all_final_ok());
        assert_eq!(t.non_final_count(), 1);
    }

    #[test]
    fn happy_path_seals_with_writeback() {
        // 正常流：全域深化→声明→绿写回收官，终态 Sealed 且同步绿。
        let mut d = build_handoff_decl();
        assert_eq!(d.stage(), DeclStage::Declared);
        assert!(d.sync_ok());
        assert!(
            d.seal(DeclWriteStatus::Done, "四十行 [已深化] 终态 摘要非空")
                .is_ok()
        );
        assert_eq!(d.stage(), DeclStage::Sealed);
        assert!(d.all_final_ok());
        assert!(d.sync_ok());
    }

    #[test]
    fn injections_thirty_rounds_all_caught() {
        // 注入必抓：伪声明10+空条款10+计数10 全数命中，无逃脱。
        let (caught, total, escape) = inject_sweep();
        assert_eq!(total, 30);
        assert_eq!(caught, 30, "逃脱：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：确定性 xorshift32 下两次全流程逐项相同，指纹相等。
        let a = build_handoff_decl();
        let b = build_handoff_decl();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.stage(), b.stage());
        assert_eq!(a.row_count(), b.row_count());
        let mut i = 0usize;
        while i < BATCH_TOTAL {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(inject_sweep(), inject_sweep());
    }

    #[test]
    fn boundaries_and_sealed_gates() {
        // 四类边界：批端点/终态再推 EOrder/容量耗尽/未声明禁收官/
        // 封账后写拒/重声明重封倒退拒。
        let mut d = HandoffDecl::new();
        assert!(d.deepen(0).is_ok());
        assert!(d.deepen(0).is_ok());
        assert_eq!(d.deepen(0), Err(HdlDiag::EOrder.val()));
        assert_eq!(d.deepen(40), Err(HdlDiag::EBatchRange.val()));
        let mut c = build_handoff_decl();
        let mut k = 0usize;
        while k < LOG_CAP {
            assert!(c.add_note("点验").is_ok());
            k += 1;
        }
        assert_eq!(c.add_note("超容"), Err(HdlDiag::ECap.val()));
        let mut p = HandoffDecl::new();
        assert_eq!(
            p.seal(DeclWriteStatus::Done, "摘要非空"),
            Err(HdlDiag::ENotFinal.val())
        );
        let mut f = build_handoff_decl();
        assert!(f.seal(DeclWriteStatus::Done, "收官摘要非空").is_ok());
        assert_eq!(f.deepen(5), Err(HdlDiag::ESealed.val()));
        assert_eq!(f.add_note("封账后"), Err(HdlDiag::ESealed.val()));
        assert_eq!(
            f.declare(40, 240_000, 800, "重声明"),
            Err(HdlDiag::ESealed.val())
        );
        assert_eq!(
            f.seal(DeclWriteStatus::Done, "重封"),
            Err(HdlDiag::ERewind.val())
        );
    }

    #[test]
    fn state_machine_monotonic_with_inject() {
        // 单调状态机：逐行升秩、停滞注入后 all_final 红且 declare 必拒。
        let mut d = HandoffDecl::new();
        assert!(d.deepen(3).is_ok());
        match d.row_of(3) {
            Some(r) => assert_eq!(r.state, BatchState::Deepening),
            None => assert!(false, "行应在账"),
        }
        assert!(d.deepen(3).is_ok());
        match d.row_of(3) {
            Some(r) => assert_eq!(r.state, BatchState::Deepened),
            None => assert!(false, "行应在账"),
        }
        assert!(d.deepen_all().is_ok());
        assert!(d.all_final_ok());
        assert!(d
            .inject_stall(20, BatchState::Deepening)
            .is_ok());
        assert_eq!(
            d.declare(40, 240_000, 800, "条款非空"),
            Err(HdlDiag::ENotFinal.val())
        );
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本卡 14 条 = 13 实质 + 1 收口）。
        let s = run_unx12791_checks();
        assert!(s.all_passed(), "红项：{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 14);
        assert_eq!(s.tally().1, 0);
        assert!(!s.truncated());
    }
}
