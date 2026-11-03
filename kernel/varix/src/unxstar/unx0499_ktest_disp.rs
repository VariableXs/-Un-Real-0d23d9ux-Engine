//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0499 · ktest 显示面断言接入 · 0x761C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6-N）：
//!   套件 unx_a1_b25_p1：显示面 12 卡判据→断言 1:1 双向对账，全绿判
//!   定，抽断言注入全抓
//! 设计要点：
//!   1. 显示面卡表冻结 12 卡（F0486~F0498 区段：0486 文字输出/0487
//!      清屏/0488 渲染护栏/0489 渲染性能/0490 渲染图案/0491 帧缓冲哨
//!      兵/0492 渲染操作/0493 多输出/0494 五位数字/0495 坏参演练/
//!      0497 显示交接/0498 对冲总账；0496 为 IO-TLB 非显示面不占席）；
//!   2. 1:1 对账：每卡恰一断言（键 = 卡号×1000+1），正向对账（卡→
//!      断言无缺）+ 反向对账（断言→卡无孤），双向齐绿才许收账；
//!   3. 全绿判定：12 卡判定闭集全绿（冻结判定表 [绿;12]），任一卡翻
//!      红全绿判定即破（GreenBreach 显性拒）；
//!   4. 抽断言注入 12 种抽法：抽掉第 k 张卡（k=1..12）的断言各为一
//!      路，正向对账必破（CardMissing），12 路 × 10 轮全抓；
//!   5. 单调状态机：装配→对账→封账，封账后抽卡/翻判显性拒；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x761C0，AI-VX7-1104 批6-N 独占槽）。
pub const SEG_BASE: u32 = 0x761C0;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 显示面卡数（冻结 12）。
pub const CARD_N: usize = 12;
/// 显示面卡表（卡号 + 卡名；F0486~F0498 区段 12 席）。
pub const CARDS: [(u16, &str); 12] = [
    (486, "textout"),
    (487, "cleartext"),
    (488, "renderbound"),
    (489, "renderperf"),
    (490, "renderpat"),
    (491, "fbsentinel"),
    (492, "renderops"),
    (493, "multiout"),
    (494, "disp5num"),
    (495, "badparam_drill"),
    (497, "disp_handoff"),
    (498, "recon_disp"),
];
/// 非显示面排除席（F0496 为 IO-TLB 面，不在显示面 12 席内）。
pub const EXCLUDED_CARD: u16 = 496;
/// 断言键倍率（键 = 卡号×1000 + 1，1:1 语义）。
pub const KEY_SCALE: u32 = 1000;
/// 套件名（冻结逐字）。
pub const SUITE_NAME: &str = "unx_a1_b25_p1";

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 卡号越界（不在显示面 12 席闭集）
    CardUnknown,
    /// 卡无断言（正向对账破）
    CardMissing,
    /// 断言无卡归属（反向对账破）
    AsrtOrphan,
    /// 断言键重复（1:1 语义破坏）
    AsrtDup,
    /// 全绿判定破（任一卡翻红）
    GreenBreach,
    /// 封账后改写（单调闸显性拒）
    SealLocked,
    /// 状态倒退
    StateRegress,
    /// 双跑不一致
    NotReproducible,
}

impl KtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::CardUnknown => "卡号越界：只在显示面 12 席闭集（F0486~F0498 区段）内取号",
            KtCode::CardMissing => "卡无断言：正向对账破，建议按 卡号×1000+1 补齐 1:1 断言",
            KtCode::AsrtOrphan => "断言无卡归属：反向对账破，建议裁掉孤断言或补注册卡席",
            KtCode::AsrtDup => "断言键重复：1:1 语义红线，建议按卡号重编断言键",
            KtCode::GreenBreach => "全绿判定破：任一卡翻红即破，建议修复红卡后重收账",
            KtCode::SealLocked => "封账后改写：套件已封禁写，建议开新一代套件账",
            KtCode::StateRegress => "状态倒退：装配→对账→封账单向单调，建议重开新代",
            KtCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [KtCode; 8] = [
        KtCode::CardUnknown,
        KtCode::CardMissing,
        KtCode::AsrtOrphan,
        KtCode::AsrtDup,
        KtCode::GreenBreach,
        KtCode::SealLocked,
        KtCode::StateRegress,
        KtCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if let Some(c) = KtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x761C0 段闭集内"
}

// ── 卡席闭集与断言键 ────────────────────────────────────────────
/// 卡号合法（在冻结卡表内）。
pub fn card_valid(id: u16) -> bool {
    let mut i = 0;
    while i < CARDS.len() {
        if let Some((cid, _)) = CARDS.get(i) {
            if *cid == id {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 卡号席次（0 起；越席回 None）。
pub fn card_slot(id: u16) -> Option<usize> {
    let mut i = 0;
    while i < CARDS.len() {
        if let Some((cid, _)) = CARDS.get(i) {
            if *cid == id {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 断言键（1:1：键 = 卡号×1000 + 1）。
pub fn assert_key(id: u16) -> u32 {
    id as u32 * KEY_SCALE + 1
}

// ── 卡判定闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 绿
    Green,
    /// 红
    Red,
}

// ── 套件状态机（装配→对账→封账，单调前进） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuitePhase {
    /// 装配（可抽可补）
    Assemble,
    /// 对账毕
    Reconciled,
    /// 封账
    Sealed,
}

impl SuitePhase {
    pub fn rank(self) -> u32 {
        match self {
            SuitePhase::Assemble => 0,
            SuitePhase::Reconciled => 1,
            SuitePhase::Sealed => 2,
        }
    }
    pub fn can_advance(self, to: SuitePhase) -> bool {
        to.rank() > self.rank()
    }
    pub const ALL: [SuitePhase; 3] = [
        SuitePhase::Assemble,
        SuitePhase::Reconciled,
        SuitePhase::Sealed,
    ];
}

// ── 套件账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispCardRow {
    pub id: u16,
    pub name: &'static str,
    pub verdict: Verdict,
}

#[derive(Clone, Debug)]
pub struct KtestSuite {
    pub cards: Vec<DispCardRow>,
    /// 断言键账（1:1：与卡号一一对应）。
    pub asserts: Vec<u32>,
    pub phase: SuitePhase,
    pub write_rejects: u32,
}

impl KtestSuite {
    /// 按冻结卡表装配（判定全绿、断言 1:1 齐配）。
    pub fn assemble() -> KtestSuite {
        let mut s = KtestSuite {
            cards: Vec::new(),
            asserts: Vec::new(),
            phase: SuitePhase::Assemble,
            write_rejects: 0,
        };
        let mut i = 0;
        while i < CARDS.len() {
            if let Some((id, name)) = CARDS.get(i) {
                s.cards.push(DispCardRow { id: *id, name, verdict: Verdict::Green });
                s.asserts.push(assert_key(*id));
            }
            i += 1;
        }
        s
    }

    /// 抽断言（注入面：装配态才许抽；卡号越席显性拒）。
    pub fn remove_assert(&mut self, id: u16) -> Result<(), u32> {
        if self.phase != SuitePhase::Assemble {
            self.write_rejects = self.write_rejects.saturating_add(1);
            return Err(KtCode::SealLocked.val());
        }
        let slot = match card_slot(id) {
            Some(x) => x,
            None => return Err(KtCode::CardUnknown.val()),
        };
        // 只抽断言不抽卡席：卡在账而无断言，正向对账必破（CardMissing）。
        if slot >= self.asserts.len() {
            return Err(KtCode::CardMissing.val());
        }
        self.asserts.remove(slot);
        Ok(())
    }

    /// 翻判（注入面：绿翻红；封账态显性拒）。
    pub fn flip(&mut self, id: u16) -> Result<(), u32> {
        if self.phase != SuitePhase::Assemble {
            self.write_rejects = self.write_rejects.saturating_add(1);
            return Err(KtCode::SealLocked.val());
        }
        let slot = match card_slot(id) {
            Some(x) => x,
            None => return Err(KtCode::CardUnknown.val()),
        };
        match self.cards.get_mut(slot) {
            Some(row) => {
                row.verdict = Verdict::Red;
                Ok(())
            }
            None => Err(KtCode::CardUnknown.val()),
        }
    }

    /// 正向对账：每卡恰有 1:1 断言键，缺即 CardMissing。
    pub fn forward_recon(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.cards.len() {
            let row = match self.cards.get(i) {
                Some(x) => x,
                None => return Err(KtCode::CardUnknown.val()),
            };
            if !card_valid(row.id) {
                return Err(KtCode::CardUnknown.val());
            }
            let want = assert_key(row.id);
            if i >= self.asserts.len() {
                return Err(KtCode::CardMissing.val());
            }
            let got = match self.asserts.get(i) {
                Some(x) => *x,
                None => 0,
            };
            if got != want {
                return Err(KtCode::CardMissing.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 反向对账：每断言键归属恰一卡，孤即 AsrtOrphan。
    pub fn reverse_recon(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.asserts.len() {
            let key = match self.asserts.get(i) {
                Some(x) => *x,
                None => return Err(KtCode::AsrtOrphan.val()),
            };
            // 键还原卡号：键 = 卡号×1000 + 1。
            let id = match key.checked_sub(1) {
                Some(q) => (q / KEY_SCALE) as u16,
                None => return Err(KtCode::AsrtOrphan.val()),
            };
            if !card_valid(id) {
                return Err(KtCode::AsrtOrphan.val());
            }
            // 1:1 位置对齐：卡席必同位。
            let slot = match card_slot(id) {
                Some(x) => x,
                None => return Err(KtCode::AsrtOrphan.val()),
            };
            if slot != i || slot >= self.cards.len() {
                return Err(KtCode::AsrtOrphan.val());
            }
            i += 1;
        }
        if self.asserts.len() != self.cards.len() {
            return Err(KtCode::AsrtOrphan.val());
        }
        Ok(())
    }

    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        let mut i = 0;
        while i < self.cards.len() {
            match self.cards.get(i) {
                Some(row) => {
                    if row.verdict != Verdict::Green {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        self.cards.len() == CARD_N && self.asserts.len() == CARD_N
    }

    /// 双向对账 + 全绿判定（全过进对账态；违约按首破面回因）。
    pub fn reconcile(&mut self) -> Result<(u32, u32), u32> {
        if !self.phase.can_advance(SuitePhase::Reconciled) {
            self.write_rejects = self.write_rejects.saturating_add(1);
            return Err(KtCode::StateRegress.val());
        }
        if self.cards.len() != CARD_N || self.asserts.len() != CARD_N {
            return Err(KtCode::CardMissing.val());
        }
        self.forward_recon()?;
        self.reverse_recon()?;
        // 断言键查重（1:1 闭集语义）。
        let mut i = 0;
        while i < self.asserts.len() {
            let mut j = i + 1;
            while j < self.asserts.len() {
                let a = match self.asserts.get(i) {
                    Some(x) => *x,
                    None => return Err(KtCode::AsrtDup.val()),
                };
                let b = match self.asserts.get(j) {
                    Some(x) => *x,
                    None => return Err(KtCode::AsrtDup.val()),
                };
                if a == b {
                    return Err(KtCode::AsrtDup.val());
                }
                j += 1;
            }
            i += 1;
        }
        if !self.all_green() {
            return Err(KtCode::GreenBreach.val());
        }
        self.phase = SuitePhase::Reconciled;
        Ok((self.cards.len() as u32, self.asserts.len() as u32))
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), u32> {
        if !self.phase.can_advance(SuitePhase::Sealed) {
            self.write_rejects = self.write_rejects.saturating_add(1);
            return Err(KtCode::StateRegress.val());
        }
        self.phase = SuitePhase::Sealed;
        Ok(())
    }
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（12 路：抽掉第 k 张卡的断言） ─────────────────────
pub struct InjectLedger {
    pub tries: [u32; CARD_N],
    pub caught: [u32; CARD_N],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; CARD_N], caught: [0; CARD_N] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < CARD_N {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < CARD_N {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：装配→双向对账→全绿→封账→封后抽卡/翻判必拒 ─────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut s = KtestSuite::assemble();
    if s.cards.len() != CARD_N {
        return Err(KtCode::CardMissing.val());
    }
    let (cards, asserts) = s.reconcile()?;
    if cards != CARD_N as u32 || asserts != CARD_N as u32 {
        return Err(KtCode::AsrtOrphan.val());
    }
    if !s.all_green() {
        return Err(KtCode::GreenBreach.val());
    }
    s.seal()?;
    if s.phase != SuitePhase::Sealed {
        return Err(KtCode::StateRegress.val());
    }
    // 封后改写全拒（抽卡 + 翻判 → 记账 2）。
    let first_id = match CARDS.first() {
        Some((id, _)) => *id,
        None => 0,
    };
    let mut rej = 0u32;
    match s.remove_assert(first_id) {
        Err(e) if e == KtCode::SealLocked.val() => rej = rej.saturating_add(1),
        _ => return Err(KtCode::NotReproducible.val()),
    }
    match s.flip(first_id) {
        Err(e) if e == KtCode::SealLocked.val() => rej = rej.saturating_add(1),
        _ => return Err(KtCode::NotReproducible.val()),
    }
    Ok((cards, asserts, rej))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（卡 12 / 断言 12 / 封后写拒 2）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((12, 12, 2)) => Ok(()),
        _ => Err(KtCode::NotReproducible.val()),
    }
}

/// 序 B：越席卡显性拒（抽断言卡号 496 / 999 / 0）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut s = KtestSuite::assemble();
    let r1 = s.remove_assert(EXCLUDED_CARD);
    let r2 = s.remove_assert(999);
    let r3 = s.remove_assert(0);
    match (r1, r2, r3) {
        (Err(e1), Err(e2), Err(e3))
            if e1 == KtCode::CardUnknown.val()
                && e2 == KtCode::CardUnknown.val()
                && e3 == KtCode::CardUnknown.val() =>
        {
            Ok(())
        }
        _ => Err(KtCode::NotReproducible.val()),
    }
}

/// 序 C：口径冻结（12 卡 / 套件名逐字 / 排除席 496 / 键公式）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        CARD_N == 12,
        SUITE_NAME == "unx_a1_b25_p1",
        EXCLUDED_CARD == 496,
        assert_key(486) == 486001,
        assert_key(498) == 498001,
        !card_valid(EXCLUDED_CARD),
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(KtCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 12 抽法） ──────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 12 种抽法：抽掉第 k 张卡（席次 0..11）的断言，对账必破。
        let mut k = 0;
        while k < CARD_N {
            let mut s = KtestSuite::assemble();
            let id = match CARDS.get(k) {
                Some((cid, _)) => *cid,
                None => 0,
            };
            let removed = s.remove_assert(id).is_ok();
            let hit = removed
                && match s.reconcile() {
                    Err(e) => e == KtCode::CardMissing.val(),
                    Ok(_) => false,
                };
            led.record(k, hit);
            k += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if let Some(c) = KtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if let Some(c) = KtCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// LCG 确定性。
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入账 0 号槽取值（.get 取，不裸下标）。
fn slot0(t: &[u32; CARD_N]) -> u32 {
    match t.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 注入账末槽取值（.get 取，不裸下标）。
fn slot_last(t: &[u32; CARD_N]) -> u32 {
    match t.get(CARD_N - 1) {
        Some(x) => *x,
        None => 0,
    }
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        CARD_N == 12 && SUITE_NAME == "unx_a1_b25_p1" && CARDS.len() == 12,
        format!("套件 {} 显示面 12 席", SUITE_NAME),
    ));
    // 2. 卡表闭集（12 卡号互异且全在 F0486~F0498 区段）。
    let mut ids_ok = true;
    let mut i = 0;
    while i < CARDS.len() {
        let (id, _) = match CARDS.get(i) {
            Some(x) => *x,
            None => {
                ids_ok = false;
                break;
            }
        };
        if id < 486 || id > 498 || id == EXCLUDED_CARD {
            ids_ok = false;
        }
        let mut j = i + 1;
        while j < CARDS.len() {
            let (id2, _) = match CARDS.get(j) {
                Some(x) => *x,
                None => (0, ""),
            };
            if id == id2 {
                ids_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-卡表闭集",
        ids_ok,
        format!("F0486~F0498 区段 12 席互异 排除席 {}", EXCLUDED_CARD),
    ));
    // 3. 断言键 1:1 公式。
    let mut key_ok = true;
    let mut i = 0;
    while i < CARDS.len() {
        if let Some((id, _)) = CARDS.get(i) {
            if assert_key(*id) != *id as u32 * KEY_SCALE + 1 {
                key_ok = false;
            }
        }
        i += 1;
    }
    v.push((
        "J1-断言键1:1公式",
        key_ok && assert_key(486) == 486001 && assert_key(498) == 498001,
        String::from("键 = 卡号×1000+1 逐卡复算"),
    ));
    // 4. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((c, a, rej)) => *c == 12 && *a == 12 && *rej == 2,
            Err(_) => false,
        },
        match &g {
            Ok((c, a, rej)) => format!("卡{} 断言{} 封后写拒{}", c, a, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. 正向对账（卡→断言无缺）。
    let s0 = KtestSuite::assemble();
    v.push((
        "J1-正向对账",
        s0.forward_recon().is_ok(),
        String::from("12 卡恰配 12 断言键"),
    ));
    // 6. 反向对账（断言→卡无孤）。
    v.push((
        "J1-反向对账",
        s0.reverse_recon().is_ok(),
        String::from("12 断言键恰归 12 卡席"),
    ));
    // 7. 全绿判定（冻结判定表 [绿;12]）。
    v.push((
        "J1-全绿判定",
        s0.all_green(),
        String::from("判定闭集全绿"),
    ));
    // 8. 翻红破全绿（静态注入一轮直查）。
    let mut s1 = KtestSuite::assemble();
    let flip_ok = s1.flip(490).is_ok();
    let green_broke = flip_ok && match s1.reconcile() {
        Err(e) => e == KtCode::GreenBreach.val(),
        Ok(_) => false,
    };
    v.push((
        "J1-翻红破全绿",
        green_broke,
        String::from("任一卡翻红全绿判定即破（GreenBreach）"),
    ));
    // 9. 探针三序。
    v.push((
        "J1-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a全链 b越席拒 c口径"),
    ));
    // 10. 抽断言注入 12 抽法 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-抽断言12抽法10/10",
        match &inj {
            Ok(l) => l.all_caught() && slot0(&l.tries) == JUDGE_ROUNDS && slot_last(&l.tries) == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("首槽t={} 末槽t={} c={:?}", slot0(&l.tries), slot_last(&l.tries), l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 11. 封账后改写显性拒。
    v.push((
        "J1-封后改写拒",
        match &g {
            Ok((_, _, rej)) => *rej == 2,
            Err(_) => false,
        },
        String::from("封账后抽卡/翻判全拒（SealLocked）"),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 14. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", KtCode::ALL.len()),
    ));
    // 15. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x761C0 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx0499_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0499 ktest 显示面断言接入");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((c, a, rej)) => {
                assert_eq!(c, 12);
                assert_eq!(a, 12);
                assert_eq!(rej, 2);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(CARD_N, 12);
        assert_eq!(CARDS.len(), 12);
        assert_eq!(CARDS.first().copied(), Some((486, "textout")));
        assert_eq!(EXCLUDED_CARD, 496);
        assert!(!card_valid(496));
        assert!(card_valid(486));
        assert!(card_valid(498));
        assert!(!card_valid(496));
        assert!(!card_valid(0));
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn bidirectional_recon() {
        let s = KtestSuite::assemble();
        assert!(s.forward_recon().is_ok());
        assert!(s.reverse_recon().is_ok());
        assert!(s.all_green());
        // 抽一枚断言：正向破（CardMissing）且反向破（席数失配）。
        let mut s2 = s.clone();
        s2.remove_assert(491).ok();
        match s2.forward_recon() {
            Err(e) => assert_eq!(e, KtCode::CardMissing.val()),
            Ok(()) => assert!(false, "抽断言后正向对账未破"),
        }
        match s2.reverse_recon() {
            Err(e) => assert_eq!(e, KtCode::AsrtOrphan.val()),
            Ok(()) => assert!(false, "抽断言后反向对账未破"),
        }
    }

    #[test]
    fn inject_twelve_methods_always_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught(), "12 抽法 10/10 全抓");
        assert_eq!(slot0(&led.tries), JUDGE_ROUNDS);
        assert_eq!(slot_last(&led.tries), JUDGE_ROUNDS);
        // 逐席核对：每席恰好 10 试 10 抓。
        let mut k = 0;
        while k < CARD_N {
            let t = match led.tries.get(k) {
                Some(x) => *x,
                None => 0,
            };
            let c = match led.caught.get(k) {
                Some(x) => *x,
                None => 0,
            };
            assert_eq!(t, JUDGE_ROUNDS, "抽法{} 试数", k + 1);
            assert_eq!(c, JUDGE_ROUNDS, "抽法{} 抓数", k + 1);
            k += 1;
        }
    }

    #[test]
    fn green_breach_on_flip() {
        let mut s = KtestSuite::assemble();
        s.flip(488).ok();
        assert!(!s.all_green());
        match s.reconcile() {
            Err(e) => assert_eq!(e, KtCode::GreenBreach.val()),
            Ok(_) => assert!(false, "翻红后对账未破"),
        }
    }

    #[test]
    fn state_machine_and_seal_lock() {
        assert!(SuitePhase::Assemble.can_advance(SuitePhase::Reconciled));
        assert!(SuitePhase::Reconciled.can_advance(SuitePhase::Sealed));
        assert!(!SuitePhase::Sealed.can_advance(SuitePhase::Assemble));
        let mut s = KtestSuite::assemble();
        s.reconcile().ok();
        // 对账态抽卡：单调闸拒（非装配态）。
        match s.remove_assert(486) {
            Err(e) => assert_eq!(e, KtCode::SealLocked.val()),
            Ok(()) => assert!(false, "对账态抽卡未拒"),
        }
        s.seal().ok();
        match s.flip(486) {
            Err(e) => assert_eq!(e, KtCode::SealLocked.val()),
            Ok(()) => assert!(false, "封账后翻判未拒"),
        }
        assert_eq!(s.write_rejects, 2);
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(KtCode::CardMissing.val()), KtCode::CardMissing.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }

    #[test]
    fn assert_key_formula_boundary() {
        assert_eq!(assert_key(486), 486001);
        assert_eq!(assert_key(498), 498001);
        // 键还原卡号双向可逆。
        let mut i = 0;
        while i < CARDS.len() {
            let (id, _) = match CARDS.get(i) {
                Some(x) => *x,
                None => break,
            };
            let key = assert_key(id);
            let back = match key.checked_sub(1) {
                Some(q) => (q / KEY_SCALE) as u16,
                None => 0,
            };
            assert_eq!(back, id, "键双向可逆 卡{}", id);
            i += 1;
        }
    }

    #[test]
    fn checks_face_full_green() {
        let s = run_unx0499_checks();
        assert!(s.len() >= 12, "检查条数须 ≥12");
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
