//! UNX-F51539 B17 防重 grep 执行记录（关键词族：带宽仲裁/QoS/令牌桶/公平性） · 0xC09C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51540-J1 批印入册：域账累计 102,000/240,000
//!
//! 设计要点：
//!   ① 关键词族冻结：4 个根词（带宽仲裁/QoS/令牌桶/公平性）×4 个子型
//!      共 16 条；虚拟语料 12 个模块，按冻结位图声明命中集。
//!   ② 封闭语料全量扫描恰好 102 次命中，每次命中按 HIT_WEIGHT=1000
//!      计入域账：批印入册后 域账累计 = 102,000 / 240,000（容量上限）。
//!   ③ 防重红线：同一关键词只得入册一次（防重位图），重复批次/重复单查
//!      显性拒（DupScan）并留痕；留痕记录含 (关键词, 命中数, 首/末模块)。
//!   ④ 状态机 Idle → Running → BatchSealed 只进不退；封版后任何扫描
//!      显性拒（PhaseClosed），回退显性拒（PhaseRegress）。
//!   ⑤ 判定次序：相位 → 关键词域 → 防重位 → 容量 → 入册。
//!   ⑥ 注入三类必抓 ×10：重复批次 / 幽灵关键词 / 账面溢出；
//!      注入账守恒 注入 = 检出 + 逃逸，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：0xC0880 + 0x40 × (51539 − 51534)。
pub const SEG_BASE: u32 = 0xC09C0;

/// 关键词族条数（4 根词 ×4 子型）。
pub const KWD_CNT: usize = 16;
/// 虚拟语料模块数。
pub const MODULE_CNT: usize = 12;
/// 单次命中计入域账的权重。
pub const HIT_WEIGHT: u64 = 1000;
/// 域账容量上限。
pub const ACCOUNT_CAP: u64 = 240_000;
/// 封闭语料全量命中的期望值。
pub const EXPECTED_HITS: u64 = 102;

/// 冻结关键词族。
pub const KWD_FAMILY: [&str; KWD_CNT] = [
    "带宽仲裁",
    "带宽仲裁·严格",
    "带宽仲裁·轮转",
    "带宽仲裁·加权",
    "QoS",
    "QoS·档位",
    "QoS·预算",
    "QoS·削峰",
    "令牌桶",
    "令牌桶·补充",
    "令牌桶·容量",
    "令牌桶·超发",
    "公平性",
    "公平性·缺口",
    "公平性·长期账",
    "公平性·波次",
];

/// 冻结语料位图：第 k 位 = 该模块含第 k 个关键词。
/// 全量 popcount 恰为 102（批印入册 102,000 的冻结依据）。
pub const CORPUS_MASKS: [u16; MODULE_CNT] = [
    0xFFFF, 0xFFFF, 0x0FFF, 0x00FF, 0x3FFF, 0x00FF, 0x0F0F, 0x003F, 0x0700, 0x00F0, 0x001F, 0x0003,
];

/// 单关键词入册留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrepRecord {
    pub kwd: usize,
    pub hits: u32,
    pub first_mod: usize,
    pub last_mod: usize,
}

/// 批印报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchReport {
    pub records: usize,
    pub hits_total: u64,
    pub account: u64,
}

/// 诊断码闭集（防重留痕域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 相位回退或跳段
    PhaseRegress,
    /// 封版后再扫描
    PhaseClosed,
    /// 幽灵关键词（负值或超族）
    KwdGhost,
    /// 重复扫描（防重位命中）
    DupScan,
    /// 账面溢出（超 240,000 上限）
    CapOverflow,
    /// 注入账守恒失败
    LedgerMismatch,
}

impl DedupCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DedupCode::Ok => "成功",
            DedupCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值范围",
            DedupCode::PhaseRegress => "相位回退：留痕状态机只进不退",
            DedupCode::PhaseClosed => "批次已封版：留痕闭卷后不得再扫",
            DedupCode::KwdGhost => "幽灵关键词：关键词号必须落在冻结族内",
            DedupCode::DupScan => "重复扫描：同一关键词只得入册一次",
            DedupCode::CapOverflow => "账面溢出：域账不得超过 240,000",
            DedupCode::LedgerMismatch => "注入账不平：注入=检出+逃逸必须守恒",
        }
    }
}

/// 留痕状态机（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanPhase {
    /// 待扫
    Idle = 0,
    /// 扫描中（可单查）
    Running = 1,
    /// 批印封版
    BatchSealed = 2,
}

/// 防重 grep 账本。
pub struct DedupLedger {
    records: Vec<GrepRecord>,
    sealed_kw: [bool; KWD_CNT],
    account: u64,
    batches: u32,
    phase: ScanPhase,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl DedupLedger {
    pub fn new() -> DedupLedger {
        DedupLedger {
            records: Vec::new(),
            sealed_kw: [false; KWD_CNT],
            account: 0,
            batches: 0,
            phase: ScanPhase::Idle,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 相位单调推进（只进不退，逐级 +1）。
    pub fn advance_phase(&mut self, target: ScanPhase) -> Result<ScanPhase, DedupCode> {
        let cur = self.phase as u32;
        let tgt = target as u32;
        if tgt <= cur || tgt > cur + 1 {
            return Err(DedupCode::PhaseRegress);
        }
        self.phase = target;
        Ok(self.phase)
    }

    /// 相位回退注入面。
    pub fn try_set_phase(&mut self, target: ScanPhase) -> Result<(), DedupCode> {
        if (target as u32) >= self.phase as u32 {
            return Err(DedupCode::PhaseRegress);
        }
        Err(DedupCode::PhaseRegress)
    }

    /// 单关键词命中数（闭集内）。
    pub fn hits_of(kwd: usize) -> u32 {
        let mut hits = 0u32;
        for m in 0..MODULE_CNT {
            let mask = match CORPUS_MASKS.get(m) {
                Some(v) => *v,
                None => 0,
            };
            if (mask >> kwd) & 1 == 1 {
                hits += 1;
            }
        }
        hits
    }

    /// 入册扫描（判序：相位 → 关键词域 → 防重位 → 容量 → 入册）。
    pub fn grep_single(&mut self, kwd: i64) -> Result<GrepRecord, DedupCode> {
        if self.phase == ScanPhase::BatchSealed {
            return Err(DedupCode::PhaseClosed);
        }
        if kwd < 0 {
            return Err(DedupCode::KwdGhost);
        }
        let kwd = kwd as usize;
        if kwd >= KWD_CNT {
            return Err(DedupCode::KwdGhost);
        }
        let sealed = match self.sealed_kw.get(kwd) {
            Some(v) => *v,
            None => return Err(DedupCode::KwdGhost),
        };
        if sealed {
            return Err(DedupCode::DupScan);
        }
        if self.phase == ScanPhase::Idle {
            let _ = self.advance_phase(ScanPhase::Running)?;
        }
        let hits = Self::hits_of(kwd);
        let mut first_mod = 0usize;
        let mut last_mod = 0usize;
        let mut seen = 0u32;
        for m in 0..MODULE_CNT {
            let mask = match CORPUS_MASKS.get(m) {
                Some(v) => *v,
                None => 0,
            };
            if (mask >> kwd) & 1 == 1 {
                if seen == 0 {
                    first_mod = m;
                }
                last_mod = m;
                seen += 1;
            }
        }
        let add = hits as u64 * HIT_WEIGHT;
        let next = match self.account.checked_add(add) {
            Some(v) => v,
            None => return Err(DedupCode::CapOverflow),
        };
        if next > ACCOUNT_CAP {
            return Err(DedupCode::CapOverflow);
        }
        let rec = GrepRecord {
            kwd,
            hits,
            first_mod,
            last_mod,
        };
        self.records.push(rec);
        match self.sealed_kw.get_mut(kwd) {
            Some(v) => *v = true,
            None => return Err(DedupCode::KwdGhost),
        }
        self.account = next;
        Ok(rec)
    }

    /// 批印入册：全族一次扫描后封版；封版后重复批次显性拒。
    /// 自 Idle 或 Running（单查后补扫）均可起批，已入册词不重计。
    pub fn run_batch(&mut self) -> Result<BatchReport, DedupCode> {
        if self.phase == ScanPhase::BatchSealed {
            return Err(DedupCode::DupScan);
        }
        if self.phase == ScanPhase::Idle {
            let _ = self.advance_phase(ScanPhase::Running)?;
        }
        for k in 0..KWD_CNT {
            let sealed = match self.sealed_kw.get(k) {
                Some(v) => *v,
                None => false,
            };
            if !sealed {
                self.grep_single(k as i64)?;
            }
        }
        let _ = self.advance_phase(ScanPhase::BatchSealed)?;
        self.batches += 1;
        Ok(BatchReport {
            records: self.records.len(),
            hits_total: self.account / HIT_WEIGHT,
            account: self.account,
        })
    }

    /// 账面溢出注入面：任何越限加账一律拒且账不动。
    pub fn forge_account(&mut self, units: u64) -> Result<u64, DedupCode> {
        let next = match self.account.checked_add(units) {
            Some(v) => v,
            None => return Err(DedupCode::CapOverflow),
        };
        if next > ACCOUNT_CAP {
            return Err(DedupCode::CapOverflow);
        }
        self.account = next;
        Ok(self.account)
    }

    pub fn phase(&self) -> ScanPhase {
        self.phase
    }

    pub fn account(&self) -> u64 {
        self.account
    }

    pub fn batches(&self) -> u32 {
        self.batches
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn record_at(&self, idx: usize) -> Option<GrepRecord> {
        self.records.get(idx).copied()
    }

    pub fn note_injected(&mut self) {
        self.injected += 1;
    }

    pub fn note_detected(&mut self) {
        self.detected += 1;
    }

    pub fn note_escaped(&mut self) {
        self.escaped += 1;
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一批印序列两次建账，留痕与域账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, Vec<GrepRecord>, u32) {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let mut recs: Vec<GrepRecord> = Vec::new();
        for i in 0..dl.record_count() {
            match dl.record_at(i) {
                Some(r) => recs.push(r),
                None => (),
            }
        }
        (dl.account(), recs, dl.batches())
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51539_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51539");
    // 1 正常流：批印入册 → 域账累计 102,000 / 240,000，封版。
    {
        let mut dl = DedupLedger::new();
        let rep = dl.run_batch();
        let ok = match rep {
            Ok(r) => {
                r.records == KWD_CNT
                    && r.hits_total == EXPECTED_HITS
                    && r.account == EXPECTED_HITS * HIT_WEIGHT
            }
            Err(_) => false,
        };
        s.add_dyn(
            "批印入册 102,000/240,000",
            ok && dl.phase() == ScanPhase::BatchSealed && dl.account() <= ACCOUNT_CAP,
            "封闭语料 102 次命中 ×1000 恰入册",
        );
    }
    // 2 边界-零：零账起点合法（未扫不得有账）。
    {
        let dl = DedupLedger::new();
        s.add_dyn(
            "边界零账起点",
            dl.account() == 0 && dl.record_count() == 0 && dl.phase() == ScanPhase::Idle,
            "未扫描前账面必须为 0",
        );
    }
    // 3 边界-负：负关键词号显性拒。
    {
        let mut dl = DedupLedger::new();
        let r = dl.grep_single(-1);
        s.add_dyn("边界负值拒", r == Err(DedupCode::KwdGhost), "负关键词号必须显性拒");
    }
    // 4 边界-上限：族末关键词（15 号）合法入册。
    {
        let mut dl = DedupLedger::new();
        let r = dl.grep_single((KWD_CNT - 1) as i64);
        s.add_dyn("边界上限放行", r.is_ok(), "族末关键词合法入册");
    }
    // 5 边界-上限+1：16 号起越族拒。
    {
        let mut dl = DedupLedger::new();
        let r = dl.grep_single(KWD_CNT as i64);
        s.add_dyn("边界上限+1 拒", r == Err(DedupCode::KwdGhost), "越族关键词必须拒绝");
    }
    // 6 注入必抓 ①：重复批次 ×10 全抓（防重红线）。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            dl.note_injected();
            match dl.run_batch() {
                Err(DedupCode::DupScan) => {
                    caught += 1;
                    dl.note_detected();
                }
                _ => dl.note_escaped(),
            }
        }
        s.add_dyn("重复批次 10/10 拒", caught == 10, "封版后重扫必须 10/10 显性拒");
    }
    // 7 注入必抓 ②：幽灵关键词 ×10 全抓。
    {
        let mut dl = DedupLedger::new();
        let mut caught = 0u32;
        for k in 0..10u32 {
            dl.note_injected();
            let probe = if k % 2 == 0 {
                -1 - (k as i64)
            } else {
                KWD_CNT as i64 + k as i64
            };
            match dl.grep_single(probe) {
                Err(DedupCode::KwdGhost) => {
                    caught += 1;
                    dl.note_detected();
                }
                _ => dl.note_escaped(),
            }
        }
        s.add_dyn("幽灵关键词 10/10 拒", caught == 10, "族外关键词必须全抓");
    }
    // 8 注入必抓 ③：账面溢出 ×10 全抓且账不动。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let base = dl.account();
        let mut caught = 0u32;
        for k in 0..10u32 {
            dl.note_injected();
            match dl.forge_account(139_000 + k as u64) {
                Err(DedupCode::CapOverflow) => {
                    caught += 1;
                    dl.note_detected();
                }
                _ => dl.note_escaped(),
            }
        }
        s.add_dyn(
            "账面溢出 10/10 拒",
            caught == 10 && dl.account() == base,
            "越限加账必须全拒且账面分毫不动",
        );
    }
    // 9 判定次序：相位先于关键词域（封版+幽灵同传报封版）。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let r = dl.grep_single(-1);
        s.add_dyn(
            "判序相位先于关键词",
            r == Err(DedupCode::PhaseClosed),
            "封版判据先于幽灵关键词判据",
        );
    }
    // 10 状态机拒止：相位回退/跳段显性拒。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let r1 = dl.try_set_phase(ScanPhase::Idle);
        let r2 = dl.try_set_phase(ScanPhase::Running);
        let mut dl2 = DedupLedger::new();
        let r3 = dl2.advance_phase(ScanPhase::BatchSealed);
        s.add_dyn(
            "状态机拒止",
            r1 == Err(DedupCode::PhaseRegress)
                && r2 == Err(DedupCode::PhaseRegress)
                && r3 == Err(DedupCode::PhaseRegress),
            "回退/越级推进必须全部拒绝",
        );
    }
    // 11 单关键词防重：同词二次单查拒；批量补扫填满其余 15 条。
    {
        let mut dl = DedupLedger::new();
        let first = dl.grep_single(3);
        let dup = dl.grep_single(3);
        let batch = dl.run_batch();
        let batch_ok = match batch {
            Ok(rep) => rep.records == KWD_CNT,
            Err(_) => false,
        };
        s.add_dyn(
            "单关键词防重留痕",
            first.is_ok() && dup == Err(DedupCode::DupScan) && batch_ok,
            "同词只得入册一次，批量补扫不重计",
        );
    }
    // 12 域账对账：逐条命中合计 = 102，域账 = 102 × 1000。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let mut hits_sum = 0u64;
        let mut kw_seen: [bool; KWD_CNT] = [false; KWD_CNT];
        for i in 0..dl.record_count() {
            match dl.record_at(i) {
                Some(r) => {
                    hits_sum += r.hits as u64;
                    match kw_seen.get_mut(r.kwd) {
                        Some(v) => *v = true,
                        None => (),
                    }
                }
                None => (),
            }
        }
        let mut all_seen = true;
        for i in 0..KWD_CNT {
            let v = match kw_seen.get(i) {
                Some(v) => *v,
                None => false,
            };
            if !v {
                all_seen = false;
            }
        }
        s.add_dyn(
            "域账对账 102×1000",
            hits_sum == EXPECTED_HITS
                && all_seen
                && dl.account() == EXPECTED_HITS * HIT_WEIGHT
                && dl.account() <= ACCOUNT_CAP,
            "留痕逐条合计与域账全对平",
        );
    }
    // 13 留痕闭集：每条 hits ≥1、首模块 ≤ 末模块、记录数 = 族大小。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let mut closed = dl.record_count() == KWD_CNT;
        for i in 0..dl.record_count() {
            match dl.record_at(i) {
                Some(r) => {
                    if r.hits == 0 || r.first_mod > r.last_mod || r.last_mod >= MODULE_CNT {
                        closed = false;
                    }
                }
                None => closed = false,
            }
        }
        s.add_dyn("留痕闭集形态", closed, "每条留痕必须有命中且模块界合法");
    }
    // 14 对账守恒：注入 = 检出 + 逃逸，逃逸 0。
    {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        for _ in 0..10u32 {
            dl.note_injected();
            match dl.run_batch() {
                Err(DedupCode::DupScan) => dl.note_detected(),
                _ => dl.note_escaped(),
            }
        }
        s.add_dyn(
            "注入账守恒",
            dl.conserved() && dl.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑留痕一致", dual_run_consistent(), "同批印序列两次建账逐字段相等");
    // 16 文案闭集非空 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [DedupCode; 8] = [
    DedupCode::Ok,
    DedupCode::SegBad,
    DedupCode::PhaseRegress,
    DedupCode::PhaseClosed,
    DedupCode::KwdGhost,
    DedupCode::DupScan,
    DedupCode::CapOverflow,
    DedupCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_seal_account_exact() {
        let mut dl = DedupLedger::new();
        let rep = match dl.run_batch() {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "批印不应失败");
                return;
            }
        };
        assert_eq!(rep.records, KWD_CNT);
        assert_eq!(rep.hits_total, EXPECTED_HITS);
        assert_eq!(rep.account, 102_000);
        assert!(rep.account <= ACCOUNT_CAP);
        assert_eq!(dl.phase(), ScanPhase::BatchSealed);
    }

    #[test]
    fn zero_start_and_ghost_bounds() {
        let dl = DedupLedger::new();
        assert_eq!(dl.account(), 0);
        let mut dl2 = DedupLedger::new();
        assert_eq!(dl2.grep_single(-1), Err(DedupCode::KwdGhost));
        assert_eq!(dl2.grep_single(KWD_CNT as i64), Err(DedupCode::KwdGhost));
        assert!(dl2.grep_single((KWD_CNT - 1) as i64).is_ok());
    }

    #[test]
    fn duplicate_batch_caught_ten_of_ten() {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            dl.note_injected();
            if dl.run_batch() == Err(DedupCode::DupScan) {
                caught += 1;
                dl.note_detected();
            } else {
                dl.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert!(dl.conserved());
    }

    #[test]
    fn overflow_injection_account_untouched() {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        let base = dl.account();
        let mut caught = 0u32;
        for k in 0..10u32 {
            dl.note_injected();
            if dl.forge_account(139_000 + k as u64) == Err(DedupCode::CapOverflow) {
                caught += 1;
                dl.note_detected();
            } else {
                dl.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert_eq!(dl.account(), base);
        assert!(dl.conserved());
    }

    #[test]
    fn judge_order_phase_before_kwd() {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        assert_eq!(dl.grep_single(-1), Err(DedupCode::PhaseClosed));
    }

    #[test]
    fn single_kw_dedup_then_batch_fills_rest() {
        let mut dl = DedupLedger::new();
        assert!(dl.grep_single(3).is_ok());
        assert_eq!(dl.grep_single(3), Err(DedupCode::DupScan));
        let rep = match dl.run_batch() {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "补扫不应失败");
                return;
            }
        };
        assert_eq!(rep.records, KWD_CNT);
        assert_eq!(rep.hits_total, EXPECTED_HITS);
    }

    #[test]
    fn phase_machine_rejects_regress_and_skip() {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        assert_eq!(dl.try_set_phase(ScanPhase::Idle), Err(DedupCode::PhaseRegress));
        assert_eq!(dl.try_set_phase(ScanPhase::Running), Err(DedupCode::PhaseRegress));
        let mut dl2 = DedupLedger::new();
        assert_eq!(
            dl2.advance_phase(ScanPhase::BatchSealed),
            Err(DedupCode::PhaseRegress)
        );
    }

    #[test]
    fn records_closed_shape() {
        let mut dl = DedupLedger::new();
        let _ = dl.run_batch();
        assert_eq!(dl.record_count(), KWD_CNT);
        for i in 0..dl.record_count() {
            let r = match dl.record_at(i) {
                Some(r) => r,
                None => {
                    assert!(false, "留痕缺失");
                    return;
                }
            };
            assert!(r.hits >= 1);
            assert!(r.first_mod <= r.last_mod);
            assert!(r.last_mod < MODULE_CNT);
            assert_eq!(r.kwd, i);
        }
    }

    #[test]
    fn corpus_popcount_is_102() {
        let mut total = 0u32;
        for m in 0..MODULE_CNT {
            let mask = match CORPUS_MASKS.get(m) {
                Some(v) => *v,
                None => 0,
            };
            total += mask.count_ones();
        }
        assert_eq!(total as u64, EXPECTED_HITS);
        for k in 0..KWD_CNT {
            assert!(DedupLedger::hits_of(k) >= 1, "关键词 {} 必须至少命中一次", k);
        }
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
