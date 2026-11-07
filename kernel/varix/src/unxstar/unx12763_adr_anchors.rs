//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12763 · 版本锚 ADR-UNX-008 全批复核 · 0x828C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12764-J1 待实测清单终账齐（项/批号/依据三列），
//!      虚构注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 批版本锚登记账：B00~B16 共 17 个 VX9 批次（F12600~F12799）
//!     加 23 个既有批次，登记行闭集恰 40 行，一行一批不许缺批；
//!   - 每行三列：项（锚项名非空）/批号（批次 id 区间）/依据（锚出处档），
//!     与待实测清单终账「项/批号/依据」三列同构——版本锚复核的产出
//!     就是待实测清单的依据列底账；
//!   - 锚型闭集三档（ADR-UNX-008 §2）：1=主锚 2=复核锚 3=遗留锚；
//!     依据档闭集三档（ADR-UNX-008 附录）：1=冻结主表 2=复核附页
//!     3=既有台账附录；档外值一律判虚构，零容忍；
//!   - 批次一致性守卫：id 区间落在 VX9 窗（≥12600）的批次必配主锚/
//!     复核锚加冻结主表/复核附页，窗外的既有批次必配遗留锚加台账附录，
//!     错配即判红（AnchorPairBreak）——这是虚构注入的主要抓捕面；
//!   - 单调状态机：登记 Registering → 复核 Reviewing → 写回 Closed，
//!     倒退显性拒（AnchorRegression）并给出中文操作建议；
//!   - 注入三族：缺锚（锚型=0）/虚构档（闭集外）/状态机倒退，
//!     LCG（种子=0x828C0）各 10 轮 10/10 必抓；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x828C0，VX9 批公式 0x80000+0x40×(12763−12600)）。
pub const SEG_BASE: u32 = 0x828C0;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 登记账容量：40 批（ADR-UNX-008 §1：VX9 17 批 + 既有 23 批恰满）。
pub const BATCH_CAP: usize = 40;
/// VX9 批次下沿（DISPATCH_VX9.md 批次表：B00 起 12600）。
pub const VX9_ID_FLOOR: u32 = 12600;
/// VX9 批次上沿（B16 止 12799）。
pub const VX9_ID_CEIL: u32 = 12799;
/// 既有批次 id 上沿（遗留台账全部 < 12600）。
pub const LEGACY_ID_CEIL: u32 = 12599;

/// 锚型闭集档数（ADR-UNX-008 §2）。
pub const ANCHOR_KIND_TIERS: usize = 3;
/// 依据档闭集档数（ADR-UNX-008 附录）。
pub const PROV_TIER_TIERS: usize = 3;

// ── 40 批冻结表（项/批号区间/锚型/依据档；出处：DISPATCH_VX9.md 批次表
//    与 ADR-UNX-008 版本锚登记册） ─────────────────────────────────
/// VX9 十七批：B00~B08 配主锚+冻结主表，B09~B16 配复核锚+复核附页。
pub const VX9_BATCHES: [(&str, u32, u32, u32, u32); 17] = [
    ("B00", 12600, 12611, 1, 1),
    ("B01", 12612, 12623, 1, 1),
    ("B02", 12624, 12635, 1, 1),
    ("B03", 12636, 12647, 1, 1),
    ("B04", 12648, 12659, 1, 1),
    ("B05", 12660, 12671, 1, 1),
    ("B06", 12672, 12683, 1, 1),
    ("B07", 12684, 12695, 1, 1),
    ("B08", 12696, 12707, 1, 1),
    ("B09", 12708, 12719, 2, 2),
    ("B10", 12720, 12731, 2, 2),
    ("B11", 12732, 12743, 2, 2),
    ("B12", 12744, 12755, 2, 2),
    ("B13", 12756, 12767, 2, 2),
    ("B14", 12768, 12779, 2, 2),
    ("B15", 12780, 12791, 2, 2),
    ("B16", 12792, 12799, 2, 2),
];

/// 既有二十三批：锚型=遗留锚，依据=既有台账附录，id 区间全部 <12600。
pub const LEGACY_BATCHES: [(&str, u32, u32, u32, u32); 23] = [
    ("L01", 12288, 12300, 3, 3),
    ("L02", 12301, 12313, 3, 3),
    ("L03", 12314, 12326, 3, 3),
    ("L04", 12327, 12339, 3, 3),
    ("L05", 12340, 12352, 3, 3),
    ("L06", 12353, 12365, 3, 3),
    ("L07", 12366, 12378, 3, 3),
    ("L08", 12379, 12391, 3, 3),
    ("L09", 12392, 12404, 3, 3),
    ("L10", 12405, 12417, 3, 3),
    ("L11", 12418, 12430, 3, 3),
    ("L12", 12431, 12443, 3, 3),
    ("L13", 12444, 12456, 3, 3),
    ("L14", 12457, 12469, 3, 3),
    ("L15", 12470, 12482, 3, 3),
    ("L16", 12483, 12495, 3, 3),
    ("L17", 12496, 12508, 3, 3),
    ("L18", 12509, 12521, 3, 3),
    ("L19", 12522, 12534, 3, 3),
    ("L20", 12535, 12547, 3, 3),
    ("L21", 12548, 12560, 3, 3),
    ("L22", 12561, 12573, 3, 3),
    ("L23", 12574, 12586, 3, 3),
];

// ── 诊断码闭集（0x828C0 段内 0x40 窗口） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorCode {
    /// 项列空：锚项名不得为空串（三列之项）
    AnchorItemEmpty,
    /// 缺锚：锚型=0（缺锚注入主抓面）
    AnchorMissing,
    /// 锚型越闭集：3 档外的锚型值
    AnchorKindInvalid,
    /// 依据档越闭集：3 档外的依据档值
    AnchorProvInvalid,
    /// 锚型与依据档批次错配（VX9 窗/遗留窗配档颠倒）
    AnchorPairBreak,
    /// 批号区间非法（lo>hi 或跨窗）
    BatchRangeBad,
    /// 批次重复登记：一批一锚一行
    AnchorDoubleBooked,
    /// 登记账已满：40 批恰满后不再收行
    AnchorLedgerFull,
    /// 状态机倒退：Closed 不得退回 Reviewing/Registering
    AnchorRegression,
    /// 未登记齐 40 行即发起全批复核
    ReviewPremature,
    /// 复核破例：存在行缺项/缺锚/错配仍判全绿
    ReviewBreak,
}

impl AnchorCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AnchorCode::AnchorItemEmpty => "项列空：锚项名必须非空（三列之项）",
            AnchorCode::AnchorMissing => "缺锚：锚型=0，先补锚再登记（ADR-UNX-008 §2）",
            AnchorCode::AnchorKindInvalid => "锚型越闭集：仅 1=主锚 2=复核锚 3=遗留锚",
            AnchorCode::AnchorProvInvalid => "依据档越闭集：仅 1=冻结主表 2=复核附页 3=台账附录",
            AnchorCode::AnchorPairBreak => "锚型与依据档错配：VX9 批配主/复核锚，遗留批配遗留锚",
            AnchorCode::BatchRangeBad => "批号区间非法：lo≤hi 且不得跨 VX9/遗留窗",
            AnchorCode::AnchorDoubleBooked => "批次重复登记：一批一锚，改批号或并入原行",
            AnchorCode::AnchorLedgerFull => "登记账已满：40 批恰满，先复核再收口",
            AnchorCode::AnchorRegression => "状态机倒退：Closed 后不得回退，另开新账",
            AnchorCode::ReviewPremature => "未登记齐 40 行先复核：补齐批次行再复核",
            AnchorCode::ReviewBreak => "复核破例：存在缺项/缺锚/错配行，不得判全绿",
        }
    }
    pub const ALL: [AnchorCode; 11] = [
        AnchorCode::AnchorItemEmpty,
        AnchorCode::AnchorMissing,
        AnchorCode::AnchorKindInvalid,
        AnchorCode::AnchorProvInvalid,
        AnchorCode::AnchorPairBreak,
        AnchorCode::BatchRangeBad,
        AnchorCode::AnchorDoubleBooked,
        AnchorCode::AnchorLedgerFull,
        AnchorCode::AnchorRegression,
        AnchorCode::ReviewPremature,
        AnchorCode::ReviewBreak,
    ];
}

/// 诊断码闭集查表：未知码中文兜底（不 panic、不越界）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AnchorCode::ALL.len() {
        if let Some(c) = AnchorCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x828C0 段闭集内"
}

// ── 锚型/依据档中文命名闭集 ──────────────────────────────────────
/// 锚型中文名（ADR-UNX-008 §2 逐字）。
pub fn anchor_kind_name(kind: u32) -> &'static str {
    match kind {
        1 => "主锚",
        2 => "复核锚",
        3 => "遗留锚",
        _ => "锚型越闭集",
    }
}

/// 依据档中文名（ADR-UNX-008 附录逐字）。
pub fn prov_tier_name(tier: u32) -> &'static str {
    match tier {
        1 => "冻结主表",
        2 => "复核附页",
        3 => "台账附录",
        _ => "依据档越闭集",
    }
}

// ── 状态机（单调：登记→复核→写回） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    /// 登记中：40 行逐步收账
    Registering,
    /// 复核中：全批复核判定器已跑
    Reviewing,
    /// 已写回：状态=已完成、结果摘要非空（判据③）
    Closed,
}

impl AnchorState {
    pub fn rank(self) -> u32 {
        match self {
            AnchorState::Registering => 0,
            AnchorState::Reviewing => 1,
            AnchorState::Closed => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            AnchorState::Registering => "登记",
            AnchorState::Reviewing => "复核",
            AnchorState::Closed => "写回",
        }
    }
}

/// 单条登记行：三列（项/批号区间/依据）加锚型。
#[derive(Clone)]
pub struct AnchorRow {
    /// 项列：锚项名（非空）
    pub item: String,
    /// 批号列：批次 id 下沿
    pub id_lo: u32,
    /// 批号列：批次 id 上沿
    pub id_hi: u32,
    /// 锚型（1/2/3 闭集）
    pub kind: u32,
    /// 依据档（1/2/3 闭集）
    pub prov: u32,
}

/// 40 批版本锚登记账。
pub struct AnchorLedger {
    rows: [Option<AnchorRow>; BATCH_CAP],
    booked: usize,
    state: AnchorState,
    /// 写回摘要（判据③：非空才算写回）
    summary: String,
}

impl AnchorLedger {
    pub fn new() -> AnchorLedger {
        AnchorLedger {
            rows: [const { None }; BATCH_CAP],
            booked: 0,
            state: AnchorState::Registering,
            summary: String::new(),
        }
    }

    /// 批号是否重复（同 id 区间重叠即视为同批）。
    fn overlaps(&self, id_lo: u32, id_hi: u32) -> bool {
        let mut i = 0;
        while i < BATCH_CAP {
            if let Some(Some(rr)) = self.rows.get(i) {
                if !(id_hi < rr.id_lo || id_lo > rr.id_hi) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 登记一行（四类前置校验 + 批次配档一致性守卫）。
    ///
    /// 校验次序：项列 → 锚型 → 依据档 → 批号区间 → 配档 → 重复 → 容量。
    pub fn register(
        &mut self,
        item: &str,
        id_lo: u32,
        id_hi: u32,
        kind: u32,
        prov: u32,
    ) -> Result<usize, u32> {
        if self.state.rank() >= AnchorState::Reviewing.rank() {
            return Err(AnchorCode::AnchorRegression.val());
        }
        if item.is_empty() {
            return Err(AnchorCode::AnchorItemEmpty.val());
        }
        if kind == 0 {
            return Err(AnchorCode::AnchorMissing.val());
        }
        if kind > ANCHOR_KIND_TIERS as u32 {
            return Err(AnchorCode::AnchorKindInvalid.val());
        }
        if prov == 0 {
            return Err(AnchorCode::AnchorProvInvalid.val());
        }
        if prov > PROV_TIER_TIERS as u32 {
            return Err(AnchorCode::AnchorProvInvalid.val());
        }
        if id_lo > id_hi {
            return Err(AnchorCode::BatchRangeBad.val());
        }
        // 配档守卫：VX9 窗批次 ↔ 主/复核锚+冻结主表/复核附页；
        // 遗留窗批次 ↔ 遗留锚+台账附录。区间跨窗按 VX9 批从严。
        let in_vx9 = id_hi >= VX9_ID_FLOOR;
        let pair_ok = if in_vx9 {
            id_lo >= VX9_ID_FLOOR
                && id_hi <= VX9_ID_CEIL
                && (kind == 1 || kind == 2)
                && (prov == 1 || prov == 2)
                && kind == prov
        } else {
            id_hi <= LEGACY_ID_CEIL && kind == 3 && prov == 3
        };
        if !pair_ok {
            return Err(AnchorCode::AnchorPairBreak.val());
        }
        if self.overlaps(id_lo, id_hi) {
            return Err(AnchorCode::AnchorDoubleBooked.val());
        }
        if self.booked >= BATCH_CAP {
            return Err(AnchorCode::AnchorLedgerFull.val());
        }
        // 找空槽落账（不许裸下标，用 get_mut 扫）。
        let mut slot = None;
        let mut i = 0;
        while i < BATCH_CAP {
            if self.rows.get(i).map(|x| x.is_none()).unwrap_or(false) {
                slot = Some(i);
                i = BATCH_CAP;
            }
            i += 1;
        }
        let idx = match slot {
            Some(s) => s,
            None => return Err(AnchorCode::AnchorLedgerFull.val()),
        };
        let row = AnchorRow {
            item: String::from(item),
            id_lo,
            id_hi,
            kind,
            prov,
        };
        match self.rows.get_mut(idx) {
            Some(r) => {
                *r = Some(row);
            }
            None => return Err(AnchorCode::AnchorLedgerFull.val()),
        }
        self.booked += 1;
        Ok(idx)
    }

    /// 行数。
    pub fn booked(&self) -> usize {
        self.booked
    }

    /// 读行。
    pub fn row(&self, idx: usize) -> Option<&AnchorRow> {
        match self.rows.get(idx) {
            Some(Some(r)) => Some(r),
            _ => None,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> AnchorState {
        self.state
    }

    /// 写回摘要。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 单行合法判定：项列非空 + 锚型在册 + 依据档在册 + 配档一致。
    pub fn row_ok(&self, idx: usize) -> bool {
        match self.rows.get(idx) {
            Some(Some(r)) => {
                let kind_ok = r.kind >= 1 && r.kind <= ANCHOR_KIND_TIERS as u32;
                let prov_ok = r.prov >= 1 && r.prov <= PROV_TIER_TIERS as u32;
                let in_vx9 = r.id_hi >= VX9_ID_FLOOR;
                let pair_ok = if in_vx9 {
                    r.id_lo >= VX9_ID_FLOOR && r.id_hi <= VX9_ID_CEIL
                        && (r.kind == 1 || r.kind == 2) && (r.prov == 1 || r.prov == 2)
                        && r.kind == r.prov
                } else {
                    r.id_hi <= LEGACY_ID_CEIL && r.kind == 3 && r.prov == 3
                };
                !r.item.is_empty() && kind_ok && prov_ok && pair_ok
            }
            _ => false,
        }
    }

    /// 全批复核判定器：40 行齐 + 逐行三列合法。
    /// 返回通过行数；任一行破例或行数不足即 Err。
    pub fn full_review(&mut self) -> Result<u32, u32> {
        if self.state.rank() >= AnchorState::Reviewing.rank() {
            // 复核只允许从登记态发起一次；重复复核按倒退拒。
            return Err(AnchorCode::AnchorRegression.val());
        }
        if self.booked != BATCH_CAP {
            return Err(AnchorCode::ReviewPremature.val());
        }
        let mut pass = 0u32;
        let mut i = 0;
        while i < BATCH_CAP {
            if self.row_ok(i) {
                pass += 1;
            }
            i += 1;
        }
        if pass != BATCH_CAP as u32 {
            return Err(AnchorCode::ReviewBreak.val());
        }
        self.state = AnchorState::Reviewing;
        Ok(pass)
    }

    /// 写回收口（判据③）：复核通过后写状态=已完成与摘要。
    pub fn close(&mut self, summary: &str) -> Result<(), u32> {
        if self.state != AnchorState::Reviewing {
            if self.state.rank() > AnchorState::Reviewing.rank() {
                return Err(AnchorCode::AnchorRegression.val());
            }
            return Err(AnchorCode::ReviewPremature.val());
        }
        if summary.is_empty() {
            return Err(AnchorCode::AnchorItemEmpty.val());
        }
        self.summary = String::from(summary);
        self.state = AnchorState::Closed;
        Ok(())
    }

    /// 写回完成判定：Closed 且摘要非空。
    pub fn writeback_ok(&self) -> bool {
        self.state == AnchorState::Closed && !self.summary.is_empty()
    }
}

// ── 确定性 LCG（种子=SEG_BASE，禁 rand） ─────────────────────────
pub struct AnchorLcg(u64);

impl AnchorLcg {
    pub const fn new(seed: u64) -> AnchorLcg {
        AnchorLcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
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

// ── 冻结表装载：把 40 批冻结表登记进账（黄金正演用） ─────────────
pub fn load_frozen_table(ledger: &mut AnchorLedger) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut i = 0;
    while i < VX9_BATCHES.len() {
        if let Some((name, lo, hi, kind, prov)) = VX9_BATCHES.get(i) {
            let item = format!("版本锚-{}", name);
            ledger.register(&item, *lo, *hi, *kind, *prov)?;
            n += 1;
        }
        i += 1;
    }
    let mut j = 0;
    while j < LEGACY_BATCHES.len() {
        if let Some((name, lo, hi, kind, prov)) = LEGACY_BATCHES.get(j) {
            let item = format!("版本锚-{}", name);
            ledger.register(&item, *lo, *hi, *kind, *prov)?;
            n += 1;
        }
        j += 1;
    }
    Ok(n)
}

// ── 注入账本：族计数守恒（注入=检出+漏检，漏检=0） ───────────────
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
                    if c != t {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
    /// 守恒判定：每族 注入=检出+漏检 且 漏检=0（等价 caught==tries）。
    pub fn conservation_ok(&self) -> bool {
        self.all_caught()
    }
}

/// 三族注入演练：缺锚/虚构档/状态机倒退，各 10 轮 LCG 驱动。
pub fn run_injections(ledger: &mut InjectLedger, rounds: u32) {
    let mut rng = AnchorLcg::new(SEG_BASE as u64);
    // 族 0：缺锚（锚型=0）——每轮构造一笔必被拒的登记。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = ledger.tries.get_mut(0) {
            *t += 1;
        }
        let mut m = AnchorLedger::new();
        let batch = (rng.below(8) as u32) + 1;
        let verdict = m.register("缺锚注入行", VX9_ID_FLOOR + batch, VX9_ID_FLOOR + batch, 0, 1);
        if verdict == Err(AnchorCode::AnchorMissing.val()) {
            if let Some(c) = ledger.caught.get_mut(0) {
                *c += 1;
            }
        }
        r += 1;
    }
    // 族 1：虚构档（依据档/锚型闭集外或错配）——LCG 随机造档外值。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = ledger.tries.get_mut(1) {
            *t += 1;
        }
        let mut m = AnchorLedger::new();
        // 造一个 4..=52 的档外值（闭集仅 1..=3），必然越闭集。
        let bad = 4 + (rng.below(49) as u32);
        let verdict = m.register("虚构注入行", VX9_ID_FLOOR, VX9_ID_FLOOR, 1, bad);
        let caught = verdict == Err(AnchorCode::AnchorProvInvalid.val());
        // 再造锚型档外值（VX9 批配档外锚型）。
        let bad_kind = 4 + (rng.below(49) as u32);
        let v2 = m.register("虚构注入行2", VX9_ID_FLOOR, VX9_ID_FLOOR, bad_kind, 1);
        if caught && v2 == Err(AnchorCode::AnchorKindInvalid.val()) {
            if let Some(c) = ledger.caught.get_mut(1) {
                *c += 1;
            }
        }
        r += 1;
    }
    // 族 2：状态机倒退——复核后回退登记必拒。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = ledger.tries.get_mut(2) {
            *t += 1;
        }
        let mut m = AnchorLedger::new();
        let _ = load_frozen_table(&mut m);
        let _ = m.full_review();
        let verdict = m.register("倒退注入行", 100, 101, 1, 1);
        if verdict == Err(AnchorCode::AnchorRegression.val()) {
            if let Some(c) = ledger.caught.get_mut(2) {
                *c += 1;
            }
        }
        r += 1;
    }
}

// ── 黄金正演：装载 40 批→全批复核→写回 → 全绿 ────────────────────
pub fn golden_run() -> Result<(u32, AnchorState), u32> {
    let mut m = AnchorLedger::new();
    let n = load_frozen_table(&mut m)?;
    if n != BATCH_CAP {
        return Err(AnchorCode::ReviewPremature.val());
    }
    let pass = m.full_review()?;
    let summary = format!("UNX-F12763 全批复核 {}/40 绿", pass);
    m.close(&summary)?;
    Ok((pass, m.state()))
}

// ── 判据谓词（私有 fn xxx_ok） ───────────────────────────────────

/// 三列终账齐：黄金账 40 行逐行 项非空+批号区间合法+依据在册。
fn three_columns_ok() -> bool {
    let mut m = AnchorLedger::new();
    if load_frozen_table(&mut m).unwrap_or(0) != BATCH_CAP {
        return false;
    }
    let mut i = 0;
    while i < BATCH_CAP {
        match m.row(i) {
            Some(r) => {
                if r.item.is_empty() || r.id_lo > r.id_hi {
                    return false;
                }
                if r.prov < 1 || r.prov > PROV_TIER_TIERS as u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 虚构注入 10/10 检出（判据②直译谓词）。
fn fabricated_drill_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, 10);
    led.tries.get(1).copied().unwrap_or(0) == 10 && led.caught.get(1).copied().unwrap_or(0) == 10
}

/// 缺锚注入 10/10 抓（域模型指派谓词）。
fn missing_anchor_drill_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, 10);
    led.tries.get(0).copied().unwrap_or(0) == 10 && led.caught.get(0).copied().unwrap_or(0) == 10
}

/// 四类边界全拒：空项/缺锚/越档/超容。
fn boundary_quad_ok() -> bool {
    let mut m = AnchorLedger::new();
    let e0 = m.register("", 12600, 12600, 1, 1) == Err(AnchorCode::AnchorItemEmpty.val());
    let e1 = m.register("边界-缺锚", 12600, 12600, 0, 1) == Err(AnchorCode::AnchorMissing.val());
    let e2 = m.register("边界-越档", 12600, 12600, 9, 1) == Err(AnchorCode::AnchorKindInvalid.val());
    // 超容：先灌满 40 行（合法遗留批行），再第 41 行必拒。
    let mut i = 0;
    while i < BATCH_CAP {
        let lo = 12288 + (i as u32) * 2;
        if m.register("边界-灌行", lo, lo + 1, 3, 3).is_err() {
            return false;
        }
        i += 1;
    }
    let e3 = m.register("边界-超容", 12600, 12601, 1, 1) == Err(AnchorCode::AnchorLedgerFull.val());
    e0 && e1 && e2 && e3
}

/// 判定次序：未登记齐先复核必须拒（ReviewPremature 先于全绿出口）。
fn verdict_order_ok() -> bool {
    let mut m = AnchorLedger::new();
    if let Some((name, lo, hi, kind, prov)) = VX9_BATCHES.get(0) {
        let item = format!("版本锚-{}", name);
        let _ = m.register(&item, *lo, *hi, *kind, *prov);
    }
    m.full_review() == Err(AnchorCode::ReviewPremature.val())
}

/// 状态机倒退显性拒（独立于注入族的显性谓词）。
fn regression_reject_ok() -> bool {
    let mut m = AnchorLedger::new();
    let _ = load_frozen_table(&mut m);
    if m.full_review().is_err() {
        return false;
    }
    if m.full_review() != Err(AnchorCode::AnchorRegression.val()) {
        return false;
    }
    let mut m2 = AnchorLedger::new();
    let _ = load_frozen_table(&mut m2);
    let _ = m2.full_review();
    let _ = m2.close("UNX-F12763 写回演练摘要");
    m2.register("写回后再登记", 100, 101, 3, 3) == Err(AnchorCode::AnchorRegression.val())
}

/// 容量恰满：40 行装满后 booked==40 且第 41 行拒。
fn capacity_exact_ok() -> bool {
    let mut m = AnchorLedger::new();
    match load_frozen_table(&mut m) {
        Ok(n) => n == BATCH_CAP && m.booked() == BATCH_CAP,
        Err(_) => false,
    }
}

/// 双跑一致：黄金正演两次结果逐字段相等。
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
    while i < AnchorCode::ALL.len() {
        if let Some(c) = AnchorCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 对账一致：冻结表行数合计 = 40 = 账容量。
fn frozen_table_aligned_ok() -> bool {
    let total = VX9_BATCHES.len().saturating_add(LEGACY_BATCHES.len());
    total == BATCH_CAP
}

/// 计数守恒：注入=检出+漏检且漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, 10);
    led.conservation_ok()
}

/// 文案闭集：全部诊断码 detail 两两互异 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let first = match AnchorCode::ALL.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let fallback = detail(SEG_BASE + 0x3F);
    if fallback.is_empty() || fallback == detail(first.val()) {
        return false;
    }
    let mut i = 0;
    while i < AnchorCode::ALL.len() {
        let mut j = i + 1;
        while j < AnchorCode::ALL.len() {
            let (a, b) = match (AnchorCode::ALL.get(i), AnchorCode::ALL.get(j)) {
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

/// 收口只增不减：诊断码闭集 11 变体齐 + 全落窗 + 命名闭集齐。
fn closure_ok() -> bool {
    if AnchorCode::ALL.len() != 11 || !codes_in_window_ok() {
        return false;
    }
    let names_ok = anchor_kind_name(1) == "主锚"
        && anchor_kind_name(2) == "复核锚"
        && anchor_kind_name(3) == "遗留锚"
        && anchor_kind_name(0) == "锚型越闭集"
        && prov_tier_name(1) == "冻结主表"
        && prov_tier_name(2) == "复核附页"
        && prov_tier_name(3) == "台账附录"
        && prov_tier_name(9) == "依据档越闭集"
        && AnchorState::Registering.name() == "登记"
        && AnchorState::Reviewing.name() == "复核"
        && AnchorState::Closed.name() == "写回";
    names_ok
}

/// 写回完成（判据③谓词）：Closed 态 + 结果摘要非空。
fn close_writeback_ok() -> bool {
    match golden_run() {
        Ok((_pass, st)) => {
            let mut m = AnchorLedger::new();
            let _ = load_frozen_table(&mut m);
            let _ = m.full_review();
            let _ = m.close("UNX-F12763 全批复核 40/40 绿");
            st == AnchorState::Closed && m.writeback_ok()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12764-J1）。
    v.push((
        "J1-待实测清单终账齐三列",
        three_columns_ok(),
        String::from("项/批号/依据 40 行逐列非空在册"),
    ));
    v.push((
        "J1-虚构注入10/10检出",
        fabricated_drill_ok(),
        String::from("档外值注入 10 轮全抓"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        close_writeback_ok(),
        String::from("Closed 态 + 摘要含 40/40"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演40批全绿",
        golden_run().is_ok(),
        String::from("装载40行→复核→写回零错"),
    ));
    // 四类边界。
    v.push((
        "边界-空项/缺锚/越档/超容",
        boundary_quad_ok(),
        String::from("ItemEmpty/Missing/KindInvalid/LedgerFull 全拒"),
    ));
    // 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led, 10);
    let t0 = led.tries.get(0).copied().unwrap_or(0);
    let c0 = led.caught.get(0).copied().unwrap_or(0);
    let t1 = led.tries.get(1).copied().unwrap_or(0);
    let c1 = led.caught.get(1).copied().unwrap_or(0);
    let t2 = led.tries.get(2).copied().unwrap_or(0);
    let c2 = led.caught.get(2).copied().unwrap_or(0);
    v.push((
        "注入-缺锚10轮全抓",
        t0 == 10 && c0 == 10 && missing_anchor_drill_ok(),
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-虚构档10轮全抓",
        t1 == 10 && c1 == 10,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        t2 == 10 && c2 == 10,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未齐先复核拒",
        verdict_order_ok(),
        String::from("1 行即复核 → ReviewPremature"),
    ));
    // 状态机倒退（独立口径）。
    v.push((
        "状态机-倒退显性拒",
        regression_reject_ok(),
        String::from("重复复核/写回后登记 → Regression"),
    ));
    // 容量恰满。
    v.push((
        "容量-40批恰满第41行拒",
        capacity_exact_ok() && boundary_quad_ok(),
        format!("BATCH_CAP={}", BATCH_CAP),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x828C0",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 对账一致。
    v.push((
        "对账一致-冻结表40行=账容量",
        frozen_table_aligned_ok(),
        String::from("VX9 17 + 遗留 23 = 40"),
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
        String::from("11 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", AnchorCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12763_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12763 版本锚 ADR-UNX-008 全批复核");
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
        let (pass, st) = g.unwrap_or((0, AnchorState::Registering));
        assert_eq!(pass, BATCH_CAP as u32);
        assert_eq!(st, AnchorState::Closed);
    }

    #[test]
    fn t02_three_columns_complete() {
        assert!(three_columns_ok());
    }

    #[test]
    fn t03_boundary_quad_rejected() {
        assert!(boundary_quad_ok());
    }

    #[test]
    fn t04_missing_anchor_injection_10_of_10() {
        assert!(missing_anchor_drill_ok());
    }

    #[test]
    fn t05_fabricated_injection_10_of_10() {
        assert!(fabricated_drill_ok());
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
        assert_eq!(SEG_BASE, 0x828C0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t09_verdict_order_premature_review() {
        assert!(verdict_order_ok());
    }

    #[test]
    fn t10_capacity_exact_and_aligned() {
        assert!(capacity_exact_ok());
        assert!(frozen_table_aligned_ok());
        assert_eq!(VX9_BATCHES.len() + LEGACY_BATCHES.len(), 40);
    }

    #[test]
    fn t11_detail_corpus_closed_and_conservation() {
        assert!(detail_corpus_closed_ok());
        assert!(count_conservation_ok());
        assert!(closure_ok());
    }

    #[test]
    fn t12_checks_entry_all_green() {
        let s = run_unx12763_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
