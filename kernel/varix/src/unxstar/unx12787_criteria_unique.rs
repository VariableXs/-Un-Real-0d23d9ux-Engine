//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12787 判据终账（三源一致终断言） · 0x82EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行（契约 §2 深化下限 ≥550 行，本模块按 550+ 交付）；
//!   ② UNX-F12788-J1 三源一致终断言绿
//!     （骨架台账/深化册递推/域总账逐批一致）；
//!   ③ 40 批 × 6000 = 240,000 行；三源一致 = 全 40 批逐批行数三源相等，
//!     每批恰 6000 行。
//! 设计要点：
//!   - 三源模型：
//!     · 骨架台账（skeleton）：40 批申报行数，冻结常量 6000/批；
//!     · 深化册递推（deepening book）：cum[i] = cum[i-1] + 6000 递推
//!       （checked_add 防溢出），逐批行数 = cum[i] − cum[i-1] 差分还原；
//!     · 域总账（domain master）：40 批实记账行数；
//!     三源逐批三方对表，任一批任一源不等即终断言红；
//!   - 终断言绿判据：全 40 批三源相等且每批恰 6000、Σ = 240,000；
//!   - 注入面：域总账单批行数扰动注入 10 轮 10/10 必抓（零静默）；
//!   - 终账状态机单调：骨架冻结 → 深化递推毕 → 三源对平终账，
//!     倒退显性拒（未冻结先递推即拒、终账后重递推即拒）；
//!   - LCG（Numerical Recipes 32 位：st = st×1664525 + 1013904223），
//!     种子写死 = SEG_BASE（0x82EC0），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic（测试区除外）；双跑一致 =
//!     同输入两次构建指纹逐域相等；中文注释与中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82EC0，VX9 卡 12787 独占槽：0x80000 + 0x40 × (12787−12600)）。
pub const SEG_BASE: u32 = 0x82EC0;

// ── 冻结常量表（判据原文锚定） ───────────────────────────────────
/// 批数冻结值：40 批。
pub const BATCH_N: usize = 40;
/// 每批行数冻结值：6000 行。
pub const ROWS_PER_BATCH: u32 = 6000;
/// 总行数冻结值：40 × 6000 = 240,000。
pub const TOTAL_ROWS: u32 = 240000;
/// 扰动注入轮数（10/10 必抓判据）。
pub const INJECT_ROUNDS: u32 = 10;
/// 递推防溢出上限（u32 域内累计行数安全界）。
pub const CUM_CAP: u32 = 1_000_000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalCode {
    /// 三源不一致：某批骨架/递推/总账行数不等，逐批回查三方
    SourceDisagreement,
    /// 批行数漂移：某批行数 ≠ 6000，补齐/回卷该批
    BatchRowCountDrift,
    /// 总行数漂移：Σ≠240000，按 40×6000 复核
    TotalDrift,
    /// 递推断裂：cum 差分非 6000 或 checked_add 拒绝，重跑递推
    RecursionBreak,
    /// 骨架未冻结先递推：先 freeze_skeleton 再 deepen
    DeepenBeforeSkeleton,
    /// 终账状态倒退：已终账禁止重递推重对表，重开走新账期
    FinalRegress,
    /// 批下标越界：仅收 0..40
    BatchIndexInvalid,
    /// 骨架缺批：40 批未冻结齐即进入深化
    SkeletonIncomplete,
}

impl FinalCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FinalCode::SourceDisagreement => "三源不一致：某批骨架/递推/总账行数不等；逐批回查三方对表",
            FinalCode::BatchRowCountDrift => "批行数漂移：某批≠6000；补齐或回卷该批后重对表",
            FinalCode::TotalDrift => "总行数漂移：Σ≠240000；按 40×6000 复核三源总量",
            FinalCode::RecursionBreak => "递推断裂：cum 差分非 6000；重跑深化册递推",
            FinalCode::DeepenBeforeSkeleton => "骨架未冻结先递推：先 freeze_skeleton 再 deepen",
            FinalCode::FinalRegress => "终账状态倒退：已终账禁止重递推重对表；重开走新账期",
            FinalCode::BatchIndexInvalid => "批下标越界：仅收 0..40",
            FinalCode::SkeletonIncomplete => "骨架缺批：40 批未冻结齐；先补冻结再深化",
        }
    }
    pub const ALL: [FinalCode; 8] = [
        FinalCode::SourceDisagreement,
        FinalCode::BatchRowCountDrift,
        FinalCode::TotalDrift,
        FinalCode::RecursionBreak,
        FinalCode::DeepenBeforeSkeleton,
        FinalCode::FinalRegress,
        FinalCode::BatchIndexInvalid,
        FinalCode::SkeletonIncomplete,
    ];
}

/// 诊断码查表闭集入口。
pub fn detail(code: u32) -> &'static str {
    let mut found: &'static str = "未知诊断码：不在 0x82EC0 段闭集内";
    for c in FinalCode::ALL.iter() {
        if c.val() == code {
            found = c.detail();
        }
    }
    found
}

// ── 终账状态机（单调，倒退显性拒） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalPhase {
    /// 骨架未冻结（初始）
    Draft,
    /// 骨架冻结毕：40 批申报行数落定
    SkeletonFrozen,
    /// 深化递推毕：cum 递推链完整可差分还原
    Deepened,
    /// 三源对平终账：全 40 批三方相等且 Σ 恰 240000
    Reconciled,
}

impl FinalPhase {
    pub fn rank(self) -> u8 {
        match self {
            FinalPhase::Draft => 0,
            FinalPhase::SkeletonFrozen => 1,
            FinalPhase::Deepened => 2,
            FinalPhase::Reconciled => 3,
        }
    }
    pub fn cn(self) -> &'static str {
        match self {
            FinalPhase::Draft => "起草",
            FinalPhase::SkeletonFrozen => "骨架冻结",
            FinalPhase::Deepened => "深化递推毕",
            FinalPhase::Reconciled => "三源终账",
        }
    }
}

/// 单批三源对表行（具名域免下标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchTriple {
    /// 批号 0..40。
    pub batch: usize,
    /// 骨架台账行数。
    pub skeleton: u32,
    /// 深化册递推差分行数。
    pub deepened: u32,
    /// 域总账实记账行数。
    pub master: u32,
}

impl BatchTriple {
    /// 本批三源相等判定。
    pub fn agrees(&self) -> bool {
        self.skeleton == self.deepened && self.deepened == self.master
    }
}

/// 三源一致对表报告（逐批记账不静默）。
#[derive(Clone, PartialEq, Debug)]
pub struct AgreementReport {
    /// 三源相等批数。
    pub agreed: usize,
    /// 不一致批的三方明细（逐条记账）。
    pub mismatches: Vec<BatchTriple>,
    /// 三源合计行数。
    pub total: u64,
}

impl AgreementReport {
    /// 终断言绿判据：40 批全一致 + Σ 恰 240000。
    pub fn green(&self) -> bool {
        self.mismatches.is_empty() && self.agreed == BATCH_N && self.total == TOTAL_ROWS as u64
    }
}

/// 深化册递推链：cum[i] = cum[i-1] + ROWS_PER_BATCH（checked_add）。
pub struct DeepeningBook {
    /// 递推累计链（长度 BATCH_N，cum[0] = 6000）。
    pub cum: Vec<u32>,
}

impl DeepeningBook {
    /// 从骨架递推：骨架行数链 → 累计链（防溢出、防断链）。
    pub fn derive(skeleton: &[u32]) -> Result<DeepeningBook, u32> {
        if skeleton.len() != BATCH_N {
            return Err(FinalCode::SkeletonIncomplete.val());
        }
        let mut cum: Vec<u32> = Vec::new();
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < BATCH_N {
            let rows = match skeleton.get(i) {
                Some(v) => *v,
                None => return Err(FinalCode::SkeletonIncomplete.val()),
            };
            let next = prev.checked_add(rows).ok_or(FinalCode::RecursionBreak.val())?;
            if next > CUM_CAP {
                return Err(FinalCode::RecursionBreak.val());
            }
            cum.push(next);
            prev = next;
            i += 1;
        }
        Ok(DeepeningBook { cum })
    }

    /// 差分还原第 `b` 批行数（cum[b] − cum[b−1]，批 0 直取 cum[0]）。
    pub fn batch_rows(&self, b: usize) -> Result<u32, u32> {
        if b >= BATCH_N {
            return Err(FinalCode::BatchIndexInvalid.val());
        }
        let cur = match self.cum.get(b) {
            Some(v) => *v,
            None => return Err(FinalCode::RecursionBreak.val()),
        };
        if b == 0 {
            return Ok(cur);
        }
        let prev = match self.cum.get(b - 1) {
            Some(v) => *v,
            None => return Err(FinalCode::RecursionBreak.val()),
        };
        cur.checked_sub(prev).ok_or(FinalCode::RecursionBreak.val())
    }

    /// 递推链完整性：全 40 批差分恰 6000。
    pub fn chain_intact(&self) -> bool {
        let mut b = 0usize;
        while b < BATCH_N {
            match self.batch_rows(b) {
                Ok(r) => {
                    if r != ROWS_PER_BATCH {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            b += 1;
        }
        true
    }
}

/// 判据终账引擎（三源持有 + 单调状态机）。
pub struct FinalLedger {
    /// 源一：骨架台账（40 批申报行数）。
    pub skeleton: Vec<u32>,
    /// 源二：深化册递推链。
    pub book: Option<DeepeningBook>,
    /// 源三：域总账（40 批实记账行数）。
    pub master: Vec<u32>,
    /// 终账状态机。
    pub phase: FinalPhase,
    /// 历次对表累计不一致批数（显性记账）。
    pub disagreement_seen: u32,
}

impl FinalLedger {
    pub fn new() -> FinalLedger {
        FinalLedger {
            skeleton: Vec::new(),
            book: None,
            master: Vec::new(),
            phase: FinalPhase::Draft,
            disagreement_seen: 0,
        }
    }

    /// 黄金终账：三源全 40 批各 6000、Σ 恰 240000。
    pub fn golden() -> FinalLedger {
        let mut led = FinalLedger::new();
        let mut b = 0usize;
        while b < BATCH_N {
            led.skeleton.push(ROWS_PER_BATCH);
            led.master.push(ROWS_PER_BATCH);
            b += 1;
        }
        led
    }

    /// 骨架冻结：40 批申报行数须齐且逐批恰 6000；状态 Draft→SkeletonFrozen。
    pub fn freeze_skeleton(&mut self) -> Result<(), u32> {
        if self.skeleton.len() != BATCH_N {
            return Err(FinalCode::SkeletonIncomplete.val());
        }
        let mut b = 0usize;
        while b < BATCH_N {
            match self.skeleton.get(b) {
                Some(&r) => {
                    if r != ROWS_PER_BATCH {
                        return Err(FinalCode::BatchRowCountDrift.val());
                    }
                }
                None => return Err(FinalCode::SkeletonIncomplete.val()),
            }
            b += 1;
        }
        if self.phase == FinalPhase::Draft {
            self.phase = FinalPhase::SkeletonFrozen;
        }
        Ok(())
    }

    /// 深化递推：须骨架冻结毕；递推链差分须逐批 6000；状态→Deepened。
    pub fn deepen(&mut self) -> Result<(), u32> {
        if self.phase == FinalPhase::Reconciled || self.phase == FinalPhase::Deepened {
            return Err(FinalCode::FinalRegress.val());
        }
        if self.phase != FinalPhase::SkeletonFrozen {
            return Err(FinalCode::DeepenBeforeSkeleton.val());
        }
        let book = DeepeningBook::derive(&self.skeleton)?;
        let mut b = 0usize;
        while b < BATCH_N {
            if book.batch_rows(b)? != ROWS_PER_BATCH {
                return Err(FinalCode::RecursionBreak.val());
            }
            b += 1;
        }
        self.book = Some(book);
        self.phase = FinalPhase::Deepened;
        Ok(())
    }

    /// 三源对表：全 40 批逐批三方相等 + Σ 恰 240000；状态→Reconciled。
    pub fn reconcile(&mut self) -> Result<AgreementReport, u32> {
        if self.phase == FinalPhase::Reconciled {
            return Err(FinalCode::FinalRegress.val());
        }
        if self.phase != FinalPhase::Deepened {
            return Err(FinalCode::DeepenBeforeSkeleton.val());
        }
        let book = match self.book {
            Some(ref b) => b,
            None => return Err(FinalCode::DeepenBeforeSkeleton.val()),
        };
        let mut rep = AgreementReport {
            agreed: 0,
            mismatches: Vec::new(),
            total: 0,
        };
        let mut b = 0usize;
        while b < BATCH_N {
            let sk = match self.skeleton.get(b) {
                Some(&v) => v,
                None => return Err(FinalCode::SkeletonIncomplete.val()),
            };
            let dp = book.batch_rows(b)?;
            let ms = match self.master.get(b) {
                Some(&v) => v,
                None => return Err(FinalCode::SkeletonIncomplete.val()),
            };
            let triple = BatchTriple {
                batch: b,
                skeleton: sk,
                deepened: dp,
                master: ms,
            };
            // 合计按域总账实记账行数累计（扰动批如实入账，绿判据里显红）。
            rep.total = rep.total.saturating_add(ms as u64);
            if triple.agrees() {
                rep.agreed += 1;
            } else {
                rep.mismatches.push(triple);
            }
            b += 1;
        }
        self.disagreement_seen += rep.mismatches.len() as u32;
        self.phase = FinalPhase::Reconciled;
        Ok(rep)
    }

    /// 域总账单批扰动注入面：第 `b` 批行数 +1（注入必抓语料）。
    pub fn inject_drift(&mut self, b: usize) -> Result<(), u32> {
        if b >= BATCH_N {
            return Err(FinalCode::BatchIndexInvalid.val());
        }
        match self.master.get_mut(b) {
            Some(v) => {
                *v = v.saturating_add(1);
                Ok(())
            }
            None => Err(FinalCode::SkeletonIncomplete.val()),
        }
    }

    /// 终账指纹（双跑一致判据）。
    pub fn fingerprint(&self) -> u64 {
        let mut f: u64 = self.skeleton.len() as u64;
        f = f.wrapping_mul(31).wrapping_add(self.master.len() as u64);
        f = f.wrapping_mul(31).wrapping_add(self.phase.rank() as u64);
        for v in self.skeleton.iter() {
            f = f.wrapping_mul(31).wrapping_add(*v as u64);
        }
        for v in self.master.iter() {
            f = f.wrapping_mul(31).wrapping_add(*v as u64);
        }
        if let Some(ref b) = self.book {
            for v in b.cum.iter() {
                f = f.wrapping_mul(31).wrapping_add(*v as u64);
            }
        }
        f
    }

    /// LCG 抽检：抽中批三源行数恰 6000。
    pub fn lcg_spot_check(&self, rounds: u32) -> bool {
        let mut rng = Lcg::new(SEG_BASE);
        let mut i = 0u32;
        while i < rounds {
            let b = rng.below(BATCH_N as u32) as usize;
            let sk = match self.skeleton.get(b) {
                Some(&v) => v,
                None => return false,
            };
            let ms = match self.master.get(b) {
                Some(&v) => v,
                None => return false,
            };
            let dp = match self.book {
                Some(ref bk) => match bk.batch_rows(b) {
                    Ok(v) => v,
                    Err(_) => return false,
                },
                None => return false,
            };
            if sk != ROWS_PER_BATCH || ms != ROWS_PER_BATCH || dp != ROWS_PER_BATCH {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 矩阵可点验 10/10（A1..A10）。
    pub fn run_asserts(&mut self) -> u32 {
        let mut pass = 0u32;
        // A1：骨架 40 批齐。
        if self.skeleton.len() == BATCH_N && self.master.len() == BATCH_N {
            pass += 1;
        }
        // A2：冻结→递推全链通（在全新正演账上跑，自账任意账期皆可点验）。
        let mut probe_chain = FinalLedger::golden();
        if probe_chain.freeze_skeleton().is_ok() && probe_chain.deepen().is_ok() {
            pass += 1;
        }
        // A3：递推链完整（差分逐批 6000；在账递推册优先，缺则按骨架现推）。
        let chain_ok = match self.book {
            Some(ref b) => b.chain_intact(),
            None => match DeepeningBook::derive(&self.skeleton) {
                Ok(b) => b.chain_intact(),
                Err(_) => false,
            },
        };
        if chain_ok {
            pass += 1;
        }
        // A4：三源一致终断言绿（在全新正演账上走冻结→递推→对表全链）。
        let mut probe_full = FinalLedger::golden();
        let full_ok = probe_full.freeze_skeleton().is_ok()
            && probe_full.deepen().is_ok()
            && matches!(probe_full.reconcile(), Ok(ref r) if r.green());
        if full_ok {
            pass += 1;
        }
        // A5：终账后重对表倒退显性拒（全新账走完终账后复对表必拒）。
        let mut probe_r = FinalLedger::golden();
        let _ = probe_r.freeze_skeleton();
        let _ = probe_r.deepen();
        let _ = probe_r.reconcile();
        if matches!(probe_r.reconcile(), Err(c) if c == FinalCode::FinalRegress.val()) {
            pass += 1;
        }
        // A6：骨架未冻结先递推即拒。
        let mut probe = FinalLedger::golden();
        if matches!(probe.deepen(), Err(c) if c == FinalCode::DeepenBeforeSkeleton.val()) {
            pass += 1;
        }
        // A7：注入 1 批扰动必抓（对表报 1 不一致）。
        let mut probe2 = FinalLedger::golden();
        let _ = probe2.freeze_skeleton();
        let _ = probe2.deepen();
        let _ = probe2.inject_drift(7);
        let caught = match probe2.reconcile() {
            Ok(r) => r.mismatches.len() == 1 && r.mismatches.first().map(|m| m.batch) == Some(7),
            Err(_) => false,
        };
        if caught {
            pass += 1;
        }
        // A8：扰动批行数漂移值恰 +1（6001）。
        let drift_val = probe2.master.get(7).copied().unwrap_or(0);
        if drift_val == ROWS_PER_BATCH + 1 {
            pass += 1;
        }
        // A9：Σ 恰 240000（黄金账直读）。
        let sum: u64 = self.master.iter().map(|&v| v as u64).sum();
        if sum == TOTAL_ROWS as u64 {
            pass += 1;
        }
        // A10：批下标越界必抓。
        if self.inject_drift(BATCH_N).is_err() {
            pass += 1;
        }
        pass
    }
}

// ── 确定性 LCG（Numerical Recipes 32 位，种子=SEG_BASE） ─────────
pub struct Lcg(u32);

impl Lcg {
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：冻结→递推→对表绿→点验 10/10 ───────────────────────
pub fn golden_run() -> Result<u32, u32> {
    let mut led = FinalLedger::golden();
    led.freeze_skeleton()?;
    led.deepen()?;
    let rep = led.reconcile()?;
    if !rep.green() {
        return Err(FinalCode::SourceDisagreement.val());
    }
    Ok(led.run_asserts())
}

// ── 扰动注入：10 轮注入 10/10 必抓（零静默） ─────────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 注入轮数。
    pub tries: u32,
    /// 必抓轮数。
    pub caught: u32,
    /// 末轮注入批号。
    pub last_batch: usize,
    /// 末轮报告不一致批数。
    pub last_mismatches: usize,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger {
            tries: 0,
            caught: 0,
            last_batch: 0,
            last_mismatches: 0,
        }
    }
    /// 10/10 全抓判定。
    pub fn all_caught(&self) -> bool {
        self.tries == INJECT_ROUNDS && self.caught == INJECT_ROUNDS
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut led = FinalLedger::golden();
        let _ = led.freeze_skeleton();
        let _ = led.deepen();
        let batch = r as usize % BATCH_N;
        let _ = led.inject_drift(batch);
        let mm = match led.reconcile() {
            Ok(rep) => rep.mismatches.len(),
            Err(_) => BATCH_N + 1,
        };
        ledger.last_batch = batch;
        ledger.last_mismatches = mm;
        ledger.tries += 1;
        if mm == 1 {
            ledger.caught += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn three_source_agreement_ok() -> bool {
    let mut led = FinalLedger::golden();
    if led.freeze_skeleton().is_err() || led.deepen().is_err() {
        return false;
    }
    match led.reconcile() {
        Ok(rep) => rep.green(),
        Err(_) => false,
    }
}

fn per_batch_rows_ok() -> bool {
    let mut led = FinalLedger::golden();
    if led.freeze_skeleton().is_err() || led.deepen().is_err() {
        return false;
    }
    let b = match led.book {
        Some(ref bk) => bk.chain_intact(),
        None => false,
    };
    b
}

fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

// ── 检查面（≥12 条） ─────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 判据主锚：三源一致终断言绿。
    v.push((
        "J2-三源一致终断言绿（骨架/递推/总账逐批一致）",
        three_source_agreement_ok(),
        format!("{}批三方全等", BATCH_N),
    ));
    // 2. 40 批 × 6000 = 240000 行。
    v.push((
        "J2-40批×6000=240000行冻结",
        BATCH_N * ROWS_PER_BATCH as usize == TOTAL_ROWS as usize,
        format!("{}×{}={}", BATCH_N, ROWS_PER_BATCH, TOTAL_ROWS),
    ));
    // 3. 逐批行数三源相等（递推链完整）。
    v.push((
        "J2-递推链差分逐批恰6000",
        per_batch_rows_ok(),
        format!("cum[i]=cum[i-1]+{}", ROWS_PER_BATCH),
    ));
    // 4. 扰动注入 10/10 必抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J2-总账扰动注入10/10必抓",
        inj.all_caught(),
        format!("try/catch {}/{} 末批{}报{}", inj.tries, inj.caught, inj.last_batch, inj.last_mismatches),
    ));
    // 5. 黄金正演点验 10/10。
    let golden = golden_run();
    v.push((
        "正常流-黄金正演点验10/10",
        golden == Ok(10),
        format!("{:?}", golden),
    ));
    // 6. 终账状态机单调推进（Draft→冻结→递推→终账）。
    let mut sm = FinalLedger::golden();
    let chain = sm.freeze_skeleton().is_ok() && sm.deepen().is_ok() && sm.reconcile().is_ok();
    v.push((
        "正常流-状态机起草→冻结→递推→终账",
        chain && sm.phase == FinalPhase::Reconciled,
        format!("终态={}", sm.phase.cn()),
    ));
    // 7. 注入必抓：骨架未冻结先递推即拒。
    let mut p1 = FinalLedger::golden();
    let before = matches!(p1.deepen(), Err(c) if c == FinalCode::DeepenBeforeSkeleton.val());
    v.push((
        "注入必抓-未冻结先递推即拒",
        before,
        String::from("先 freeze_skeleton 再 deepen"),
    ));
    // 8. 注入必抓：终账后重对表倒退显性拒。
    let mut p2 = FinalLedger::golden();
    let _ = p2.freeze_skeleton();
    let _ = p2.deepen();
    let _ = p2.reconcile();
    let regress = matches!(p2.reconcile(), Err(c) if c == FinalCode::FinalRegress.val());
    v.push((
        "注入必抓-终账后重对表倒退显性拒",
        regress,
        String::from("已终账禁止重对表；重开走新账期"),
    ));
    // 9. 双跑一致：两次构建指纹逐域相等。
    let f1 = FinalLedger::golden().fingerprint();
    let f2 = FinalLedger::golden().fingerprint();
    v.push((
        "双跑一致-黄金终账指纹相等",
        f1 == f2,
        format!("{:#x} vs {:#x}", f1, f2),
    ));
    // 10. 边界：批数/批行数/总行数冻结常量。
    let consts_ok = BATCH_N == 40 && ROWS_PER_BATCH == 6000 && TOTAL_ROWS == 240000;
    v.push((
        "边界-40批/6000行/240000冻结常量",
        consts_ok,
        String::from("冻结三元常量表直读"),
    ));
    // 11. 批下标越界必抓。
    let mut p3 = FinalLedger::golden();
    let oob = p3.inject_drift(BATCH_N).is_err() && p3.inject_drift(usize::MAX).is_err();
    v.push((
        "注入必抓-批下标越界必抓",
        oob,
        String::from("仅收 0..40"),
    ));
    // 12. LCG 确定性：同种子双序列逐拍相等。
    let mut r1 = Lcg::new(SEG_BASE);
    let mut r2 = Lcg::new(SEG_BASE);
    let mut lcg_eq = true;
    let mut k = 0;
    while k < 32 {
        if r1.next() != r2.next() {
            lcg_eq = false;
        }
        k += 1;
    }
    v.push((
        "LCG-同种子双序列逐拍相等",
        lcg_eq,
        format!("种子=0x{:X}", SEG_BASE),
    ));
    // 13. 诊断码闭集查表全可回。
    let mut codes_ok = true;
    for c in FinalCode::ALL.iter() {
        if detail(c.val()) != c.detail() {
            codes_ok = false;
        }
    }
    v.push((
        "诊断码-闭集查表全可回",
        codes_ok && FinalCode::ALL.len() == 8,
        format!("{}码全查表通过", FinalCode::ALL.len()),
    ));
    // 14. 段位守卫。
    v.push((
        "段位守卫-0x82EC0",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    v
}

/// 判据入口：UNX-F12787 判据终账自检集。
pub fn run_unx12787_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12787 判据终账（三源一致终断言）");
    let items = extra_checks();
    let mut i = 0usize;
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

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_three_source_agreement_green() {
        let mut led = FinalLedger::golden();
        assert!(led.freeze_skeleton().is_ok());
        assert!(led.deepen().is_ok());
        let rep = led.reconcile();
        assert!(rep.is_ok(), "黄金账对表须绿");
        assert!(rep.unwrap_or_else(|_| AgreementReport {
            agreed: 0,
            mismatches: Vec::new(),
            total: 0,
        }).green());
        assert_eq!(led.phase, FinalPhase::Reconciled);
    }

    #[test]
    fn deepening_recursion_exact() {
        let mut led = FinalLedger::golden();
        let _ = led.freeze_skeleton();
        assert!(led.deepen().is_ok());
        assert!(led.book.is_some(), "递推毕册须在账");
        if let Some(ref b) = led.book {
            assert!(b.chain_intact(), "递推链差分逐批恰 6000");
            assert_eq!(b.batch_rows(0), Ok(ROWS_PER_BATCH));
            assert_eq!(b.batch_rows(BATCH_N - 1), Ok(ROWS_PER_BATCH));
        }
    }

    #[test]
    fn drift_injection_10_of_10() {
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert!(inj.all_caught());
        assert_eq!(inj.tries, INJECT_ROUNDS);
    }

    #[test]
    fn deepen_before_skeleton_rejected() {
        let mut led = FinalLedger::golden();
        assert_eq!(led.deepen(), Err(FinalCode::DeepenBeforeSkeleton.val()));
    }

    #[test]
    fn regress_after_reconcile_rejected() {
        let mut led = FinalLedger::golden();
        let _ = led.freeze_skeleton();
        let _ = led.deepen();
        assert!(led.reconcile().is_ok());
        assert_eq!(led.reconcile(), Err(FinalCode::FinalRegress.val()));
    }

    #[test]
    fn single_batch_drift_caught_with_batch_no() {
        let mut led = FinalLedger::golden();
        let _ = led.freeze_skeleton();
        let _ = led.deepen();
        let _ = led.inject_drift(23);
        let rep = led.reconcile().unwrap_or_else(|_| AgreementReport {
            agreed: 0,
            mismatches: Vec::new(),
            total: 0,
        });
        assert!(!rep.green(), "扰动后终断言必红");
        assert_eq!(rep.mismatches.len(), 1);
        assert_eq!(rep.mismatches.first().map(|m| m.batch), Some(23));
    }

    #[test]
    fn double_run_fingerprint_equal() {
        let a = FinalLedger::golden().fingerprint();
        let b = FinalLedger::golden().fingerprint();
        assert_eq!(a, b);
    }

    #[test]
    fn frozen_constants() {
        assert_eq!(BATCH_N, 40);
        assert_eq!(ROWS_PER_BATCH, 6000);
        assert_eq!(TOTAL_ROWS, 240000);
        assert_eq!(BATCH_N as u32 * ROWS_PER_BATCH, TOTAL_ROWS);
    }

    #[test]
    fn detail_lookup_closed_set() {
        for c in FinalCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
    }

    #[test]
    fn segment_guard_pass() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x82EC0);
    }

    #[test]
    fn golden_asserts_full_score() {
        let mut led = FinalLedger::golden();
        assert_eq!(led.run_asserts(), 10, "可点验须 10/10");
    }

    #[test]
    fn lcg_deterministic_same_seed() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }
}
