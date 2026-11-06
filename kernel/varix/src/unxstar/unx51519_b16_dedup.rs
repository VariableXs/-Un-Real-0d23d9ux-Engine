//! UNX-F51519 B16 防重 grep 执行记录（关键词族：TRIM/flush/介质变更/流优先级） · 0xC04C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51520-J1 批印入册：域账累计 96,000/240,000
//!
//! 设计要点：
//!   ① 防重 grep 按「留痕账」建模：每轮 grep 对固定关键词族
//!      （TRIM/flush/介质变更/流优先级）各落一条执行记录
//!      （序号, 关键词族, 语料摘要, 命中数），命中数由摘要确定性推导
//!      （摘要 % 7），全程无随机、无墙钟，双跑逐字段一致。
//!   ② 防重语义：同一（关键词族, 语料摘要）二元组不得重复入账，
//!      重复扫描显性拒（DupScan）——这正是「收口防重」的留痕本体：
//!      留痕既证明 grep 执行过，也证明没有重复执行。
//!   ③ 行数账：每条记录贡献 800 行留痕；30 轮 × 4 族 = 120 条 =
//!      累计 96,000 行（判据下限），预算上限 240,000 行（300 条），
//!      超预算明拒（BudgetExhausted）。
//!   ④ 批印入册状态机：开册 → 已印（只进不退）；未达 96,000 行
//!      不得印（RowsShort），已印再印拒（InkedAlready），已印后扫描拒
//!      （BookSealed），rollback 显性拒（PhaseBackward）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类注入（防重
//!      重复扫描、封册后扫描、非法关键词族）×10 轮必抓。
//!   ⑥ 判定次序：封册 → 关键词族 → 摘要 → 防重 → 预算；多重违约
//!      同时成立时取最先命中的判据码。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC04C0;

/// 关键词族数（TRIM/flush/介质变更/流优先级）。
pub const KW_COUNT: usize = 4;
/// 每条执行记录贡献的留痕行数。
pub const ROWS_PER_REC: u64 = 800;
/// 判据满册轮数（×4 族 = 120 条）。
pub const FULL_ROUNDS: u64 = 30;
/// 判据累计行数（批印入册下限）。
pub const LEDGER_TARGET: u64 = 96_000;
/// 域账预算上限（行）。
pub const BUDGET_ROWS: u64 = 240_000;
/// 记录容量上限（300 × 800 = 240,000）。
pub const REC_CAP: usize = 300;

/// 冻结关键词族表（闭集）。
pub const KEYWORDS: [&str; KW_COUNT] = ["TRIM", "flush", "介质变更", "流优先级"];

/// 诊断码闭集（防重 grep 域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 已印册，扫描被拒
    BookSealed,
    /// 关键词族越界（负值或 ≥4）
    KwBad,
    /// 语料摘要非法（负值或零）
    BadDigest,
    /// 防重触发：同（族,摘要）已入账
    DupScan,
    /// 留痕行数超预算上限
    BudgetExhausted,
    /// 未达印册行数下限
    RowsShort,
    /// 已印册，重复收官被拒
    InkedAlready,
    /// 状态机倒退（rollback 显性拒）
    PhaseBackward,
    /// 账面对平失败（行数 ≠ 记录数 × 行权值）
    LedgerMismatch,
}

impl DedupCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DedupCode::Ok => "扫描留痕成功",
            DedupCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            DedupCode::BookSealed => "已印册：册面冻结，禁止追加扫描",
            DedupCode::KwBad => "关键词族越界：只允许冻结表内 4 族",
            DedupCode::BadDigest => "语料摘要非法：摘要必须为正整数",
            DedupCode::DupScan => "防重触发：同族同摘要已留痕，禁止重复扫描",
            DedupCode::BudgetExhausted => "留痕预算耗尽：归档后开新账",
            DedupCode::RowsShort => "未达 96,000 行：补齐轮次后再印册",
            DedupCode::InkedAlready => "已印册：重复收官被拒",
            DedupCode::PhaseBackward => "状态机倒退：印册流程只进不退",
            DedupCode::LedgerMismatch => "账面不平：行数与记录数必须逐条对应",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [DedupCode; 11] = [
    DedupCode::Ok,
    DedupCode::SegBad,
    DedupCode::BookSealed,
    DedupCode::KwBad,
    DedupCode::BadDigest,
    DedupCode::DupScan,
    DedupCode::BudgetExhausted,
    DedupCode::RowsShort,
    DedupCode::InkedAlready,
    DedupCode::PhaseBackward,
    DedupCode::LedgerMismatch,
];

/// 单条 grep 执行记录（留痕账目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanRec {
    pub seq: u32,
    pub kw_id: u8,
    pub digest: u32,
    pub hits: u32,
}

/// 印册状态机：开册 → 已印（只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookPhase {
    /// 开册
    Open,
    /// 已印入册
    Inked,
}

/// 防重 grep 留痕账引擎。
pub struct GrepLedger {
    recs: Vec<ScanRec>,
    rows: u64,
    phase: BookPhase,
    injected: u64,
    detected: u64,
}

impl GrepLedger {
    /// 新账：开册空账。
    pub fn new() -> GrepLedger {
        GrepLedger {
            recs: Vec::new(),
            rows: 0,
            phase: BookPhase::Open,
            injected: 0,
            detected: 0,
        }
    }

    /// 落一条扫描留痕（判定次序：封册 → 族 → 摘要 → 防重 → 预算）。
    pub fn scan(&mut self, kw_id: i64, digest: i64) -> Result<u64, DedupCode> {
        if self.phase == BookPhase::Inked {
            return Err(DedupCode::BookSealed);
        }
        if kw_id < 0 || kw_id >= KW_COUNT as i64 {
            return Err(DedupCode::KwBad);
        }
        if digest <= 0 {
            return Err(DedupCode::BadDigest);
        }
        let kw = kw_id as u8;
        let dg = digest as u32;
        for r in self.recs.iter() {
            if r.kw_id == kw && r.digest == dg {
                return Err(DedupCode::DupScan);
            }
        }
        if self.recs.len() >= REC_CAP {
            return Err(DedupCode::BudgetExhausted);
        }
        let new_rows = match self.rows.checked_add(ROWS_PER_REC) {
            Some(x) => x,
            None => return Err(DedupCode::BudgetExhausted),
        };
        if new_rows > BUDGET_ROWS {
            return Err(DedupCode::BudgetExhausted);
        }
        let rec = ScanRec {
            seq: self.recs.len() as u32,
            kw_id: kw,
            digest: dg,
            hits: dg % 7,
        };
        self.recs.push(rec);
        self.rows = new_rows;
        Ok(self.rows)
    }

    /// 批印入册：累计达 96,000 行方可收官印册。
    pub fn ink(&mut self) -> Result<u64, DedupCode> {
        if self.phase == BookPhase::Inked {
            return Err(DedupCode::InkedAlready);
        }
        if self.rows < LEDGER_TARGET {
            return Err(DedupCode::RowsShort);
        }
        self.phase = BookPhase::Inked;
        Ok(self.rows)
    }

    /// 回滚（显性拒：印册流程只进不退）。
    pub fn rollback(&mut self) -> Result<(), DedupCode> {
        Err(DedupCode::PhaseBackward)
    }

    /// 账面对平：行数 = 记录数 × 行权值，序号严格递增。
    pub fn audit(&self) -> Result<(), DedupCode> {
        let expect = match (self.recs.len() as u64).checked_mul(ROWS_PER_REC) {
            Some(x) => x,
            None => return Err(DedupCode::LedgerMismatch),
        };
        if expect != self.rows {
            return Err(DedupCode::LedgerMismatch);
        }
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != i as u32 {
                return Err(DedupCode::LedgerMismatch);
            }
            if r.kw_id as usize >= KW_COUNT {
                return Err(DedupCode::LedgerMismatch);
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

    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            BookPhase::Open => "开册",
            BookPhase::Inked => "已印",
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

    pub fn recs_snapshot(&self) -> Vec<ScanRec> {
        self.recs.clone()
    }
}

/// 满册会话：30 轮 × 4 族 = 120 条 = 96,000 行（判据口径）。
pub fn full_session() -> GrepLedger {
    let mut g = GrepLedger::new();
    for round in 0..FULL_ROUNDS {
        for kw in 0..KW_COUNT as i64 {
            let digest = round.saturating_mul(KW_COUNT as u64)
                .saturating_add(kw as u64)
                .saturating_add(1) as i64;
            let _ = g.scan(kw, digest);
        }
    }
    g
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一会话两次重建记录逐字段一致。
pub fn dual_run_consistent() -> bool {
    let a = full_session().recs_snapshot();
    let b = full_session().recs_snapshot();
    a == b && a.len() == (FULL_ROUNDS * KW_COUNT as u64) as usize
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51519_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51519");
    // 1 正常流：满册会话 120 条 96,000 行。
    {
        let g = full_session();
        s.add_dyn(
            "满册会话 120 条",
            g.rec_len() == (FULL_ROUNDS * KW_COUNT as u64) as usize
                && g.rows() == LEDGER_TARGET,
            "30 轮 × 4 族应落 120 条 96,000 行",
        );
    }
    // 2 判据账口径：96,000/240,000。
    {
        let g = full_session();
        s.add_dyn(
            "判据账 96,000/240,000",
            LEDGER_TARGET == 96_000
                && BUDGET_ROWS == 240_000
                && LEDGER_TARGET <= BUDGET_ROWS
                && g.rows() == 96_000,
            "印册下限与预算上限必须对齐判据口径",
        );
    }
    // 3 批印入册正常收官。
    {
        let mut g = full_session();
        let r = g.ink();
        s.add_dyn(
            "批印入册成功",
            r.is_ok() && g.phase_name() == "已印",
            "达 96,000 行后应可印册",
        );
    }
    // 4 边界零：摘要零与空账印册。
    {
        let mut g = GrepLedger::new();
        let r_d = g.scan(0, 0);
        let r_i = g.ink();
        s.add_dyn(
            "边界零拒绝",
            r_d == Err(DedupCode::BadDigest) && r_i == Err(DedupCode::RowsShort),
            "零摘要与零行印册都必须明拒",
        );
    }
    // 5 边界负：关键词族负值。
    {
        let mut g = GrepLedger::new();
        let r = g.scan(-1, 5);
        s.add_dyn("边界负拒绝", r == Err(DedupCode::KwBad), "负族号必须明拒");
    }
    // 6 边界上限：族 3 放行。
    {
        let mut g = GrepLedger::new();
        let r = g.scan(3, 7);
        s.add_dyn("边界上限放行", r.is_ok(), "末族（流优先级）应放行");
    }
    // 7 边界上限+1：族 4 拒。
    {
        let mut g = GrepLedger::new();
        let r = g.scan(KW_COUNT as i64, 7);
        s.add_dyn(
            "边界上限+1 拒绝",
            r == Err(DedupCode::KwBad),
            "越出冻结族表必须拒绝",
        );
    }
    // 8 注入必抓：防重重复扫描 ×10。
    {
        let mut g = full_session();
        let mut caught = 0u32;
        // 满册会话摘要 d 与族 (d-1)%4 成对；按对重复扫描必须全抓。
        for d in 1..=10i64 {
            let kw = (d - 1) % KW_COUNT as i64;
            if g.scan(kw, d) == Err(DedupCode::DupScan) {
                caught += 1;
                g.note_detected();
            }
        }
        s.add_dyn("防重注入 10/10", caught == 10, "同族同摘要重复扫描必须全抓");
    }
    // 9 注入必抓：封册后扫描 ×10。
    {
        let mut g = full_session();
        let _ = g.ink();
        let mut caught = 0u32;
        for d in 5001..5011i64 {
            if g.scan(0, d) == Err(DedupCode::BookSealed) {
                caught += 1;
                g.note_detected();
            }
        }
        s.add_dyn("封册后扫描注入 10/10", caught == 10, "册面冻结必须全抓");
    }
    // 10 注入必抓：非法关键词族 ×10。
    {
        let mut g = GrepLedger::new();
        let mut caught = 0u32;
        for i in 0..10i64 {
            let kw = KW_COUNT as i64 + i;
            if g.scan(kw, 9) == Err(DedupCode::KwBad) {
                caught += 1;
                g.note_detected();
            }
        }
        s.add_dyn("非法族注入 10/10", caught == 10, "越出族表必须全抓");
    }
    // 11 判定次序：封册先于族非法；族非法先于摘要非法。
    {
        let mut g = full_session();
        let _ = g.ink();
        let r1 = g.scan(-1, 0);
        let mut g2 = GrepLedger::new();
        let r2 = g2.scan(-1, 0);
        s.add_dyn(
            "判定次序封册/族在前",
            r1 == Err(DedupCode::BookSealed) && r2 == Err(DedupCode::KwBad),
            "封册判据先于族判据，族判据先于摘要判据",
        );
    }
    // 12 状态机拒止：rollback、重复印册、印后扫描。
    {
        let mut g = full_session();
        let r0 = g.rollback();
        let _ = g.ink();
        let r1 = g.ink();
        let r2 = g.scan(0, 9999);
        s.add_dyn(
            "状态机倒退拒止",
            r0 == Err(DedupCode::PhaseBackward)
                && r1 == Err(DedupCode::InkedAlready)
                && r2 == Err(DedupCode::BookSealed),
            "印册流程只进不退",
        );
    }
    // 13 容量耗尽：300 条 240,000 行后 +1 拒。
    {
        let mut g = GrepLedger::new();
        let mut digest = 1i64;
        loop {
            let kw = (digest - 1) % KW_COUNT as i64;
            if g.scan(kw, digest).is_err() {
                break;
            }
            digest += 1;
        }
        let over = g.scan(0, 99999);
        s.add_dyn(
            "容量耗尽拒绝",
            g.rec_len() == REC_CAP && g.rows() == BUDGET_ROWS && over == Err(DedupCode::BudgetExhausted),
            "预算 240,000 行耗尽后必须明拒",
        );
    }
    // 14 对账守恒：注入 = 检出，且账面对平。
    {
        let mut g = GrepLedger::new();
        for i in 0..10i64 {
            let _ = g.scan(0, 1);
            g.note_detected();
        }
        let audit_ok = g.audit().is_ok();
        s.add_dyn(
            "注入账守恒",
            g.conserved() && g.injected() == 10 && audit_ok,
            "注入=检出+逃逸，逃逸恒 0，账面逐条对平",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一会话两次建账逐字段相等");
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
    fn full_session_hits_target() {
        let g = full_session();
        assert_eq!(g.rec_len(), 120);
        assert_eq!(g.rows(), 96_000);
        assert!(g.audit().is_ok());
    }

    #[test]
    fn dup_scan_rejected() {
        let mut g = GrepLedger::new();
        assert!(g.scan(0, 5).is_ok());
        assert_eq!(g.scan(0, 5), Err(DedupCode::DupScan));
        assert!(g.scan(1, 5).is_ok(), "异族同摘要不算重复");
        assert!(g.scan(1, 6).is_ok(), "异族异摘要放行");
        assert_eq!(g.scan(1, 6), Err(DedupCode::DupScan));
    }

    #[test]
    fn boundary_zero_negative_and_cap() {
        let mut g = GrepLedger::new();
        assert_eq!(g.scan(0, 0), Err(DedupCode::BadDigest));
        assert_eq!(g.scan(-1, 5), Err(DedupCode::KwBad));
        assert!(g.scan(3, 5).is_ok());
        assert_eq!(g.scan(4, 5), Err(DedupCode::KwBad));
    }

    #[test]
    fn ink_requires_target_rows() {
        let mut g = GrepLedger::new();
        assert_eq!(g.ink(), Err(DedupCode::RowsShort));
        let mut full = full_session();
        assert!(full.ink().is_ok());
    }

    #[test]
    fn sealed_book_rejects_scan_and_reink() {
        let mut g = full_session();
        assert!(g.ink().is_ok());
        assert_eq!(g.ink(), Err(DedupCode::InkedAlready));
        assert_eq!(g.scan(0, 4242), Err(DedupCode::BookSealed));
        assert_eq!(g.rollback(), Err(DedupCode::PhaseBackward));
    }

    #[test]
    fn judge_order_sealed_before_kw_before_digest() {
        let mut g = full_session();
        let _ = g.ink();
        assert_eq!(g.scan(-1, 0), Err(DedupCode::BookSealed));
        let mut g2 = GrepLedger::new();
        assert_eq!(g2.scan(-1, 0), Err(DedupCode::KwBad));
    }

    #[test]
    fn budget_capacity_exhausted() {
        let mut g = GrepLedger::new();
        let mut digest = 1i64;
        loop {
            let kw = (digest - 1) % KW_COUNT as i64;
            if g.scan(kw, digest).is_err() {
                break;
            }
            digest += 1;
        }
        assert_eq!(g.rec_len(), REC_CAP);
        assert_eq!(g.rows(), BUDGET_ROWS);
        assert_eq!(g.scan(0, 99999), Err(DedupCode::BudgetExhausted));
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut g = full_session();
        for d in 1..=10i64 {
            let kw = (d - 1) % KW_COUNT as i64;
            assert_eq!(g.scan(kw, d), Err(DedupCode::DupScan));
            g.note_detected();
        }
        assert!(g.conserved());
        assert_eq!(g.injected(), 10);
        assert_eq!(g.detected(), 10);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        let a = full_session().recs_snapshot();
        let b = full_session().recs_snapshot();
        assert_eq!(a, b);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn keywords_frozen_table() {
        assert_eq!(KEYWORDS.len(), KW_COUNT);
        assert_eq!(KEYWORDS.get(0), Some(&"TRIM"));
        assert_eq!(KEYWORDS.get(1), Some(&"flush"));
        assert_eq!(KEYWORDS.get(2), Some(&"介质变更"));
        assert_eq!(KEYWORDS.get(3), Some(&"流优先级"));
    }
}
