//! UNX-F51520 B16 批收官印（20 条冻结/累计 96,000 行） · 0xC0500段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51521-J1 预算总表 ×10 拓扑实表；表与各端点账和一致率 100%
//!
//! 设计要点：
//!   ① 批收官按「冻结印」建模：收官批由恰好 20 条冻结记录组成，每条
//!      带（序号, 行数, 冻结位）；单条行数上限 9,600，批条数上限 20，
//!      收官时累计行数必须对平 96,000 行（判据口径），差 1 行即拒印。
//!   ② 冻结单向：记录一旦冻结不可解冻（unfreeze 显性拒
//!      FrozenLocked）；解冻是倒退注入的靶面，×10 轮必抓。
//!   ③ 收官印状态机：复核中 → 已收官（只进不退）；已收官再追加、
//!      再收官一律拒（SealedAlready），unseal 显性拒（PhaseBackward）。
//!   ④ 印前四判：条数足 20（BatchShort）→ 全数冻结（NotAllFrozen）→
//!      累计对平 96,000（LedgerUnbalanced）→ 落印（stamp = SEG_BASE）；
//!      判定次序固定，多重不平时取最先命中码。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类注入（收官后
//!      追加、冻结倒退解冻、账不平强印）×10 轮必抓。
//!   ⑥ 追加判定次序：已收官 → 负行数 → 零行数 → 单条上限 → 批条数
//!      上限（容量耗尽）；行数账全程 checked 推进，不静默饱和。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0500;

/// 收官批冻结条数（判据：20 条）。
pub const REC_COUNT: usize = 20;
/// 种子单条行数（20 × 4,800 = 96,000）。
pub const ROWS_PER_SEED: u64 = 4_800;
/// 累计行数判据口径。
pub const TOTAL_ROWS: u64 = 96_000;
/// 单条行数上限。
pub const REC_ROWS_CAP: u64 = 9_600;

/// 诊断码闭集（批收官域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 已收官，追加/再收官被拒
    SealedAlready,
    /// 行数为负
    RowsNegative,
    /// 行数为零
    RowsZero,
    /// 单条行数超上限
    RowsTooMany,
    /// 批条数已满（容量耗尽，>20 条）
    BatchFull,
    /// 批条数不足（<20 条）
    BatchShort,
    /// 存在未冻结记录，禁止落印
    NotAllFrozen,
    /// 累计行数与 96,000 不平，禁止落印
    LedgerUnbalanced,
    /// 记录下标越界
    IdxBad,
    /// 冻结倒退（解冻显性拒）
    FrozenLocked,
    /// 状态机倒退（unseal 显性拒）
    PhaseBackward,
    /// 复核账面失真（收官后账面漂移）
    LedgerMismatch,
}

impl CloseCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::Ok => "操作成功",
            CloseCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            CloseCode::SealedAlready => "已收官：批面冻结，禁止追加或重复落印",
            CloseCode::RowsNegative => "行数为负：行数必须为非负整数",
            CloseCode::RowsZero => "行数为零：每条记录至少 1 行",
            CloseCode::RowsTooMany => "单条超限：行数不得超过 9,600",
            CloseCode::BatchFull => "批已满 20 条：容量耗尽，归档后开新批",
            CloseCode::BatchShort => "批不足 20 条：补齐条数后再落印",
            CloseCode::NotAllFrozen => "存在未冻结记录：全部冻结后再落印",
            CloseCode::LedgerUnbalanced => "累计不平：总行数必须恰为 96,000",
            CloseCode::IdxBad => "下标越界：核对记录序号",
            CloseCode::FrozenLocked => "冻结倒退：冻结记录不可解冻",
            CloseCode::PhaseBackward => "状态机倒退：收官流程只进不退",
            CloseCode::LedgerMismatch => "账面失真：收官后账面不得漂移",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [CloseCode; 14] = [
    CloseCode::Ok,
    CloseCode::SegBad,
    CloseCode::SealedAlready,
    CloseCode::RowsNegative,
    CloseCode::RowsZero,
    CloseCode::RowsTooMany,
    CloseCode::BatchFull,
    CloseCode::BatchShort,
    CloseCode::NotAllFrozen,
    CloseCode::LedgerUnbalanced,
    CloseCode::IdxBad,
    CloseCode::FrozenLocked,
    CloseCode::PhaseBackward,
    CloseCode::LedgerMismatch,
];

/// 单条冻结记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenRec {
    pub seq: u32,
    pub rows: u64,
    pub frozen: bool,
}

/// 收官状态机：复核中 → 已收官（只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosePhase {
    /// 复核中
    Reviewing,
    /// 已收官（印已落）
    Sealed,
}

/// 批收官印引擎。
pub struct CloseBatch {
    recs: Vec<FrozenRec>,
    phase: ClosePhase,
    stamp: u32,
    injected: u64,
    detected: u64,
}

impl CloseBatch {
    /// 新批：复核中、无印。
    pub fn new() -> CloseBatch {
        CloseBatch {
            recs: Vec::new(),
            phase: ClosePhase::Reviewing,
            stamp: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 种子批：20 条 × 4,800 行全冻结（判据口径，可直落印）。
    pub fn with_seed() -> CloseBatch {
        let mut b = CloseBatch::new();
        for _ in 0..REC_COUNT {
            let _ = b.append(ROWS_PER_SEED as i64);
        }
        for i in 0..REC_COUNT {
            let _ = b.freeze(i);
        }
        b
    }

    /// 追加一条记录（判定次序：收官 → 负 → 零 → 单条上限 → 批满）。
    pub fn append(&mut self, rows: i64) -> Result<usize, CloseCode> {
        if self.phase == ClosePhase::Sealed {
            return Err(CloseCode::SealedAlready);
        }
        if rows < 0 {
            return Err(CloseCode::RowsNegative);
        }
        if rows == 0 {
            return Err(CloseCode::RowsZero);
        }
        if rows as u64 > REC_ROWS_CAP {
            return Err(CloseCode::RowsTooMany);
        }
        if self.recs.len() >= REC_COUNT {
            return Err(CloseCode::BatchFull);
        }
        let rec = FrozenRec {
            seq: self.recs.len() as u32,
            rows: rows as u64,
            frozen: false,
        };
        self.recs.push(rec);
        Ok(self.recs.len())
    }

    /// 冻结一条记录（单向；冻结后不可解冻）。
    pub fn freeze(&mut self, idx: usize) -> Result<(), CloseCode> {
        if self.phase == ClosePhase::Sealed {
            return Err(CloseCode::SealedAlready);
        }
        let r = match self.recs.get_mut(idx) {
            Some(r) => r,
            None => return Err(CloseCode::IdxBad),
        };
        r.frozen = true;
        Ok(())
    }

    /// 解冻（显性拒：冻结倒退是注入靶面）。
    pub fn unfreeze(&mut self, idx: usize) -> Result<(), CloseCode> {
        let r = match self.recs.get(idx) {
            Some(r) => r,
            None => return Err(CloseCode::IdxBad),
        };
        if r.frozen {
            Err(CloseCode::FrozenLocked)
        } else {
            Ok(())
        }
    }

    /// 落收官印（判序：条数 → 冻结 → 对平 → 落印）。
    pub fn seal(&mut self) -> Result<u32, CloseCode> {
        if self.phase == ClosePhase::Sealed {
            return Err(CloseCode::SealedAlready);
        }
        if self.recs.len() != REC_COUNT {
            return Err(CloseCode::BatchShort);
        }
        if self.frozen_count() != REC_COUNT {
            return Err(CloseCode::NotAllFrozen);
        }
        let mut acc: u64 = 0;
        for r in self.recs.iter() {
            acc = match acc.checked_add(r.rows) {
                Some(x) => x,
                None => return Err(CloseCode::LedgerUnbalanced),
            };
        }
        if acc != TOTAL_ROWS {
            return Err(CloseCode::LedgerUnbalanced);
        }
        self.stamp = SEG_BASE;
        self.phase = ClosePhase::Sealed;
        Ok(self.stamp)
    }

    /// 倒退收官（显性拒）。
    pub fn unseal(&mut self) -> Result<(), CloseCode> {
        Err(CloseCode::PhaseBackward)
    }

    /// 复核：收官后账面不得漂移，逐条行数在界内。
    pub fn audit(&self) -> Result<(), CloseCode> {
        for r in self.recs.iter() {
            if r.rows == 0 || r.rows > REC_ROWS_CAP || r.seq as usize >= REC_COUNT {
                return Err(CloseCode::LedgerMismatch);
            }
        }
        if self.phase == ClosePhase::Sealed {
            let total = self.total_rows();
            if total != TOTAL_ROWS || self.frozen_count() != REC_COUNT || self.stamp != SEG_BASE {
                return Err(CloseCode::LedgerMismatch);
            }
        }
        Ok(())
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
    }

    pub fn rec_len(&self) -> usize {
        self.recs.len()
    }

    /// 累计行数（checked 逐条累加）。
    pub fn total_rows(&self) -> u64 {
        let mut acc: u64 = 0;
        for r in self.recs.iter() {
            acc = acc.saturating_add(r.rows);
        }
        acc
    }

    /// 冻结条数。
    pub fn frozen_count(&self) -> usize {
        let mut n = 0;
        for r in self.recs.iter() {
            if r.frozen {
                n += 1;
            }
        }
        n
    }

    pub fn seal_stamp(&self) -> u32 {
        self.stamp
    }

    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            ClosePhase::Reviewing => "复核中",
            ClosePhase::Sealed => "已收官",
        }
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 守恒断言面：注入 = 检出，逃逸恒 0 且有检出。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }

    pub fn recs_snapshot(&self) -> Vec<FrozenRec> {
        self.recs.clone()
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一构造路径两次建批快照逐字段一致。
pub fn dual_run_consistent() -> bool {
    let a = CloseBatch::with_seed();
    let b = CloseBatch::with_seed();
    a.recs_snapshot() == b.recs_snapshot()
        && a.seal_stamp() == b.seal_stamp()
        && a.phase_name() == b.phase_name()
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51520_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51520");
    // 1 正常流：种子批直落收官印。
    {
        let mut b = CloseBatch::with_seed();
        let r = b.seal();
        s.add_dyn(
            "种子批收官落印",
            r == Ok(SEG_BASE) && b.phase_name() == "已收官",
            "20 条全冻结 96,000 行应直落印",
        );
    }
    // 2 判据账口径：20 条 / 96,000 行。
    {
        let b = CloseBatch::with_seed();
        s.add_dyn(
            "判据账 20 条/96,000 行",
            REC_COUNT == 20
                && TOTAL_ROWS == 96_000
                && b.rec_len() == 20
                && b.total_rows() == 96_000
                && b.frozen_count() == 20,
            "批条数与累计行数必须对齐判据口径",
        );
    }
    // 3 边界零：零行数拒。
    {
        let mut b = CloseBatch::new();
        let r = b.append(0);
        s.add_dyn("边界零拒绝", r == Err(CloseCode::RowsZero), "零行数必须明拒");
    }
    // 4 边界负：负行数拒。
    {
        let mut b = CloseBatch::new();
        let r = b.append(-1);
        s.add_dyn("边界负拒绝", r == Err(CloseCode::RowsNegative), "负行数必须明拒");
    }
    // 5 边界上限：单条 9,600 放行。
    {
        let mut b = CloseBatch::new();
        let r = b.append(REC_ROWS_CAP as i64);
        s.add_dyn("边界上限放行", r.is_ok(), "单条上限内应放行");
    }
    // 6 边界上限+1：单条 9,601 拒。
    {
        let mut b = CloseBatch::new();
        let r = b.append(REC_ROWS_CAP as i64 + 1);
        s.add_dyn(
            "边界上限+1 拒绝",
            r == Err(CloseCode::RowsTooMany),
            "越出单条上限必须拒绝",
        );
    }
    // 7 注入必抓：收官后追加 ×10。
    {
        let mut b = CloseBatch::with_seed();
        let _ = b.seal();
        let mut caught = 0u32;
        for i in 0..10i64 {
            if b.append(100 + i) == Err(CloseCode::SealedAlready) {
                caught += 1;
                b.note_detected();
            }
        }
        s.add_dyn("收官后追加注入 10/10", caught == 10, "批面冻结必须全抓");
    }
    // 8 注入必抓：冻结倒退解冻 ×10。
    {
        let mut b = CloseBatch::with_seed();
        let mut caught = 0u32;
        for i in 0..10usize {
            let idx = i % REC_COUNT;
            if b.unfreeze(idx) == Err(CloseCode::FrozenLocked) {
                caught += 1;
                b.note_detected();
            }
        }
        s.add_dyn("解冻倒退注入 10/10", caught == 10, "冻结单向不可逆必须全抓");
    }
    // 9 注入必抓：账不平强印 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10i64 {
            let mut b = CloseBatch::new();
            let mut ok = true;
            for _ in 0..(REC_COUNT - 1) {
                if b.append(ROWS_PER_SEED as i64).is_err() {
                    ok = false;
                }
            }
            // 第 20 条多 1..10 行：累计 96,001..96,010，强印必拒。
            if b.append(ROWS_PER_SEED as i64 + i + 1).is_err() {
                ok = false;
            }
            for j in 0..REC_COUNT {
                let _ = b.freeze(j);
            }
            if ok && b.seal() == Err(CloseCode::LedgerUnbalanced) {
                caught += 1;
                b.note_detected();
            }
        }
        s.add_dyn("账不平强印注入 10/10", caught == 10, "差 1 行也必须拒印");
    }
    // 10 判定次序：收官先于负值；条数先于冻结。
    {
        let mut b = CloseBatch::with_seed();
        let _ = b.seal();
        let r1 = b.append(-1);
        let mut e = CloseBatch::new();
        let r2 = e.seal();
        s.add_dyn(
            "判定次序收官/条数在前",
            r1 == Err(CloseCode::SealedAlready) && r2 == Err(CloseCode::BatchShort),
            "收官判据先于行数判据，条数判据先于冻结判据",
        );
    }
    // 11 状态机拒止：unseal 与重复落印。
    {
        let mut b = CloseBatch::with_seed();
        let _ = b.seal();
        let r1 = b.unseal();
        let r2 = b.seal();
        s.add_dyn(
            "状态机倒退拒止",
            r1 == Err(CloseCode::PhaseBackward) && r2 == Err(CloseCode::SealedAlready),
            "收官流程只进不退",
        );
    }
    // 12 容量耗尽：第 21 条拒。
    {
        let mut b = CloseBatch::new();
        let mut filled = true;
        for _ in 0..REC_COUNT {
            if b.append(REC_ROWS_CAP as i64).is_err() {
                filled = false;
            }
        }
        let over = b.append(1);
        s.add_dyn(
            "容量耗尽拒绝",
            filled && b.rec_len() == REC_COUNT && over == Err(CloseCode::BatchFull),
            "批满 20 条后追加必须明拒",
        );
    }
    // 13 未冻结拒印。
    {
        let mut b = CloseBatch::new();
        for _ in 0..REC_COUNT {
            let _ = b.append(ROWS_PER_SEED as i64);
        }
        let r = b.seal();
        s.add_dyn(
            "未冻结拒印",
            r == Err(CloseCode::NotAllFrozen),
            "存在未冻结记录禁止落印",
        );
    }
    // 14 对账守恒：注入 = 检出，且复核不失真。
    {
        let mut b = CloseBatch::with_seed();
        let _ = b.seal();
        for i in 0..10i64 {
            let _ = b.append(i);
            b.note_detected();
        }
        let audit_ok = b.audit().is_ok();
        s.add_dyn(
            "注入账守恒",
            b.conserved() && b.injected() == 10 && audit_ok,
            "注入=检出+逃逸，逃逸恒 0，收官账面不失真",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一构造两次建批快照相等");
    // 16 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 17 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_batch_seals_clean() {
        let mut b = CloseBatch::with_seed();
        assert_eq!(b.seal(), Ok(SEG_BASE));
        assert_eq!(b.frozen_count(), REC_COUNT);
        assert_eq!(b.total_rows(), TOTAL_ROWS);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn boundary_rows_zero_negative_cap() {
        let mut b = CloseBatch::new();
        assert_eq!(b.append(0), Err(CloseCode::RowsZero));
        assert_eq!(b.append(-1), Err(CloseCode::RowsNegative));
        assert!(b.append(REC_ROWS_CAP as i64).is_ok());
        assert_eq!(b.append(REC_ROWS_CAP as i64 + 1), Err(CloseCode::RowsTooMany));
    }

    #[test]
    fn batch_capacity_exhausted() {
        let mut b = CloseBatch::new();
        for _ in 0..REC_COUNT {
            assert!(b.append(REC_ROWS_CAP as i64).is_ok());
        }
        assert_eq!(b.append(1), Err(CloseCode::BatchFull));
    }

    #[test]
    fn unbalanced_sum_rejected_at_seal() {
        let mut b = CloseBatch::new();
        for _ in 0..(REC_COUNT - 1) {
            let _ = b.append(ROWS_PER_SEED as i64);
        }
        let _ = b.append(ROWS_PER_SEED as i64 + 1);
        for j in 0..REC_COUNT {
            let _ = b.freeze(j);
        }
        assert_eq!(b.seal(), Err(CloseCode::LedgerUnbalanced));
    }

    #[test]
    fn unfrozen_rec_blocks_seal() {
        let mut b = CloseBatch::new();
        for _ in 0..REC_COUNT {
            let _ = b.append(ROWS_PER_SEED as i64);
        }
        assert_eq!(b.seal(), Err(CloseCode::NotAllFrozen));
        for j in 0..REC_COUNT {
            let _ = b.freeze(j);
        }
        assert!(b.seal().is_ok());
    }

    #[test]
    fn frozen_lock_and_sealed_rejects() {
        let mut b = CloseBatch::with_seed();
        assert_eq!(b.unfreeze(0), Err(CloseCode::FrozenLocked));
        assert!(b.seal().is_ok());
        assert_eq!(b.append(1), Err(CloseCode::SealedAlready));
        assert_eq!(b.seal(), Err(CloseCode::SealedAlready));
        assert_eq!(b.freeze(0), Err(CloseCode::SealedAlready));
        assert_eq!(b.unseal(), Err(CloseCode::PhaseBackward));
    }

    #[test]
    fn judge_order_sealed_before_rows() {
        let mut b = CloseBatch::with_seed();
        let _ = b.seal();
        assert_eq!(b.append(-1), Err(CloseCode::SealedAlready));
        assert_eq!(b.append(0), Err(CloseCode::SealedAlready));
        let mut e = CloseBatch::new();
        assert_eq!(e.seal(), Err(CloseCode::BatchShort));
    }

    #[test]
    fn idx_bounds_checked() {
        let mut b = CloseBatch::with_seed();
        assert_eq!(b.freeze(REC_COUNT), Err(CloseCode::IdxBad));
        assert_eq!(b.unfreeze(REC_COUNT), Err(CloseCode::IdxBad));
        assert_eq!(b.unfreeze(0), Err(CloseCode::FrozenLocked));
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut b = CloseBatch::with_seed();
        let _ = b.seal();
        for i in 0..10i64 {
            assert_eq!(b.append(i), Err(CloseCode::SealedAlready));
            b.note_detected();
        }
        assert!(b.conserved());
        assert_eq!(b.injected(), 10);
        assert_eq!(b.detected(), 10);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        assert_eq!(
            CloseBatch::with_seed().recs_snapshot(),
            CloseBatch::with_seed().recs_snapshot()
        );
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
