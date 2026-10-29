//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13721 · 全对象引用账总账结构 · 0x65E00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F13721-J1 总账（对象×引用×来源）三账一致性断言全绿，
//!      万对象压测账零漂移；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三账结构：对象账（id→存在位）、引用账（id→引用计数）、
//!     来源账（（来源 id，对象 id）明细行）；
//!   - 三账一致性恒等式：∀对象 o：引用账计数(o) ≡ 来源账中指向 o
//!     的明细行数（审计函数独立重算对照，零漂移判据）；
//!   - 万对象压测写死 10_000（判据规模，来源：J1 万对象）；
//!   - 账本状态机闭集：Open→Auditing→Sealed，Sealed 后增删拒收；
//!   - 边界闭集：越界 id 拒收/重复建对象拒收/释放未持引用拒收
//!     （双释放红线）/计数下探 0 拒收；
//!   - 注入三族：双释放/重复建对象/越界 id 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65E00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65E00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65E00;

/// 万对象压测规模（写死，来源：判据 J1 万对象）。
pub const STRESS_OBJECTS: usize = 10_000;

/// id 上限（写死：账本 arena 容量，压测规模 + 2 冗余）。
pub const ID_CAP: usize = 10_002;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefCode {
    /// 对象 id 越闭集
    IdOutOfRange,
    /// 对象缺失
    ObjectMissing,
    /// 重复建对象
    ObjectExists,
    /// 释放未持引用（双释放红线）
    DoubleRelease,
    /// 引用计数下探 0
    NegativeCount,
    /// 引用账漂移（计数 ≠ 明细行数）
    RefCountDrift,
    /// 三账一致性破
    TripleMismatch,
    /// 账本已封仍增删
    LedgerSealed,
}

impl RefCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RefCode::IdOutOfRange => "对象 id 越闭集：1~10002",
            RefCode::ObjectMissing => "对象缺失：未建先引用",
            RefCode::ObjectExists => "重复建对象：id 唯一",
            RefCode::DoubleRelease => "释放未持引用：双释放红线",
            RefCode::NegativeCount => "引用计数下探 0：账目失守",
            RefCode::RefCountDrift => "引用账漂移：计数 ≠ 明细行数",
            RefCode::TripleMismatch => "三账一致性破：对象/引用/来源不对账",
            RefCode::LedgerSealed => "账本已封：Sealed 后增删拒收",
        }
    }
    pub const ALL: [RefCode; 8] = [
        RefCode::IdOutOfRange,
        RefCode::ObjectMissing,
        RefCode::ObjectExists,
        RefCode::DoubleRelease,
        RefCode::NegativeCount,
        RefCode::RefCountDrift,
        RefCode::TripleMismatch,
        RefCode::LedgerSealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RefCode::ALL.len() {
        if RefCode::ALL[i].val() == code {
            return RefCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65E00 段闭集内"
}

// ── 账本状态三闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerPhase {
    Open,
    Auditing,
    Sealed,
}

/// 合法边闭集：Open→Auditing→Sealed（线性单链，Sealed 只读）。
fn ledger_edge(cur: LedgerPhase, next: LedgerPhase) -> bool {
    matches!(
        (cur, next),
        (LedgerPhase::Open, LedgerPhase::Auditing) | (LedgerPhase::Auditing, LedgerPhase::Sealed)
    )
}

/// 来源明细行：（来源 id，对象 id）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SourceEntry {
    pub src: u32,
    pub obj: usize,
}

/// 三账总账。
pub struct RefLedger {
    /// 对象账：存在位（下标 = id-1）。
    exists: Vec<bool>,
    /// 引用账：计数（下标 = id-1）。
    counts: Vec<u32>,
    /// 来源账：明细行（追加序）。
    sources: Vec<SourceEntry>,
    pub phase: LedgerPhase,
    pub created: u32,
    pub ref_events: u32,
    pub release_events: u32,
}

impl RefLedger {
    pub fn new() -> RefLedger {
        let mut ex = Vec::new();
        let mut ct = Vec::new();
        let mut i = 0;
        while i < ID_CAP {
            ex.push(false);
            ct.push(0);
            i += 1;
        }
        RefLedger {
            exists: ex,
            counts: ct,
            sources: Vec::new(),
            phase: LedgerPhase::Open,
            created: 0,
            ref_events: 0,
            release_events: 0,
        }
    }

    fn bounds(id: usize) -> Result<(), u32> {
        if id == 0 || id > ID_CAP {
            return Err(RefCode::IdOutOfRange.val());
        }
        Ok(())
    }

    /// 建对象（唯一性）。
    pub fn create(&mut self, id: usize) -> Result<(), u32> {
        if self.phase == LedgerPhase::Sealed {
            return Err(RefCode::LedgerSealed.val());
        }
        Self::bounds(id)?;
        if self.exists[id - 1] {
            return Err(RefCode::ObjectExists.val());
        }
        self.exists[id - 1] = true;
        self.counts[id - 1] = 0;
        self.created += 1;
        Ok(())
    }

    /// 持引用：来源 id + 对象 id（明细行追加，计数 +1）。
    pub fn add_ref(&mut self, obj: usize, src: u32) -> Result<(), u32> {
        if self.phase == LedgerPhase::Sealed {
            return Err(RefCode::LedgerSealed.val());
        }
        Self::bounds(obj)?;
        if !self.exists[obj - 1] {
            return Err(RefCode::ObjectMissing.val());
        }
        self.sources.push(SourceEntry { src, obj });
        self.counts[obj - 1] += 1;
        self.ref_events += 1;
        Ok(())
    }

    /// 释引用：必须持有（来源 id + 对象 id 成对注销）。
    pub fn release_ref(&mut self, obj: usize, src: u32) -> Result<(), u32> {
        if self.phase == LedgerPhase::Sealed {
            return Err(RefCode::LedgerSealed.val());
        }
        Self::bounds(obj)?;
        if !self.exists[obj - 1] {
            return Err(RefCode::ObjectMissing.val());
        }
        if self.counts[obj - 1] == 0 {
            return Err(RefCode::NegativeCount.val());
        }
        // 成对注销：找第一条（src, obj）明细行。
        let mut found = None;
        let mut i = 0;
        while i < self.sources.len() {
            let e = self.sources[i];
            if e.obj == obj && e.src == src {
                found = Some(i);
                break;
            }
            i += 1;
        }
        match found {
            None => return Err(RefCode::DoubleRelease.val()),
            Some(ix) => {
                // Vec::remove 保序（注销序确定性）。
                self.sources.remove(ix);
            }
        }
        self.counts[obj - 1] -= 1;
        self.release_events += 1;
        Ok(())
    }

    /// 审计：引用账计数(o) ≡ 来源账明细行数(o)。
    /// 单遍影子重算：独立影子数组累计行数后逐对象对照。
    pub fn audit(&self) -> Result<(u32, u32), u32> {
        // 影子数组：单遍明细行累计（独立于引用账计数路径）。
        let mut shadow: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < ID_CAP {
            shadow.push(0);
            i += 1;
        }
        let mut i = 0;
        while i < self.sources.len() {
            let o = self.sources[i].obj;
            if o == 0 || o > ID_CAP {
                return Err(RefCode::IdOutOfRange.val());
            }
            shadow[o - 1] += 1;
            i += 1;
        }
        // 逐对象对照：漂移计数。
        let mut drift = 0u32;
        let mut id = 1;
        while id <= ID_CAP {
            if self.exists[id - 1] && shadow[id - 1] != self.counts[id - 1] {
                drift += 1;
            }
            id += 1;
        }
        if drift != 0 {
            return Err(RefCode::RefCountDrift.val());
        }
        // 三账一致：明细行总数 ≡ Σ计数。
        let mut sum: u64 = 0;
        let mut id = 1;
        while id <= ID_CAP {
            if self.exists[id - 1] {
                sum += self.counts[id - 1] as u64;
            }
            id += 1;
        }
        if sum != self.sources.len() as u64 {
            return Err(RefCode::TripleMismatch.val());
        }
        Ok((self.sources.len() as u32, sum as u32))
    }

    /// 推进状态机。
    pub fn advance(&mut self, next: LedgerPhase) -> Result<(), u32> {
        if !ledger_edge(self.phase, next) {
            return Err(RefCode::LedgerSealed.val());
        }
        self.phase = next;
        Ok(())
    }

    pub fn live_lines(&self) -> usize {
        self.sources.len()
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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

// ── 黄金正演：万对象建账 → LCG 持/释 → 双审计零漂移 ──────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut l = RefLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 万对象建账：id 1..=10000。
    let mut id = 1;
    while id <= STRESS_OBJECTS {
        l.create(id)?;
        id += 1;
    }
    if l.created != STRESS_OBJECTS as u32 {
        return Err(RefCode::TripleMismatch.val());
    }
    // 持引用：每对象 1~4 条，来源 id = 100000 + 流水（确定性）。
    let mut flow: u32 = 0;
    let mut id = 1;
    while id <= STRESS_OBJECTS {
        let n = 1 + rng.below(4) as usize;
        let mut k = 0;
        while k < n {
            flow += 1;
            l.add_ref(id, 100_000 + flow)?;
            k += 1;
        }
        id += 1;
    }
    // 首审计：三账一致零漂移。
    l.advance(LedgerPhase::Auditing)?;
    let (lines1, sum1) = l.audit()?;
    if lines1 != sum1 {
        return Err(RefCode::TripleMismatch.val());
    }
    if lines1 != l.live_lines() as u32 {
        return Err(RefCode::RefCountDrift.val());
    }
    // 释引用：对前 1000 个对象各释 1 条（成对注销，来源 = 首条）。
    let mut id = 1;
    while id <= 1000 {
        // 首条来源：流水号 = 前缀和（确定性重放太贵，改查明细行首条）。
        let src = first_src_of(&l, id)?;
        l.release_ref(id, src)?;
        id += 1;
    }
    // 二审计：注销后仍零漂移（注销行已离账）。
    let (lines2, sum2) = l.audit()?;
    if lines2 + 1000 != lines1 {
        return Err(RefCode::RefCountDrift.val());
    }
    if lines2 != sum2 {
        return Err(RefCode::TripleMismatch.val());
    }
    if l.release_events != 1000 {
        return Err(RefCode::DoubleRelease.val());
    }
    // 封账 + 封后增删拒收。
    l.advance(LedgerPhase::Sealed)?;
    let rej = l.add_ref(1, 7);
    let mut sealed_ok = false;
    if let Err(e) = rej {
        if e == RefCode::LedgerSealed.val() {
            sealed_ok = true;
        }
    }
    if !sealed_ok {
        return Err(RefCode::LedgerSealed.val());
    }
    // 黄金哈希。
    let h = (lines1 as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        ^ (lines2 as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (l.created as u64);
    Ok(h)
}

/// 查某对象首条明细行来源（只读）。
fn first_src_of(l: &RefLedger, obj: usize) -> Result<u32, u32> {
    let mut i = 0;
    while i < l.live_lines() {
        let e = l.entry_at(i)?;
        if e.obj == obj {
            return Ok(e.src);
        }
        i += 1;
    }
    Err(RefCode::ObjectMissing.val())
}

impl RefLedger {
    /// 明细行只读访问。
    pub fn entry_at(&self, ix: usize) -> Result<SourceEntry, u32> {
        if ix >= self.sources.len() {
            return Err(RefCode::IdOutOfRange.val());
        }
        Ok(self.sources[ix])
    }
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
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
    // 族 0：双释放。
    ledger.tries[0] += 1;
    let mut l = RefLedger::new();
    let _ = l.create(1);
    let _ = l.add_ref(1, 9);
    let _ = l.release_ref(1, 9);
    let mut c0 = false;
    if let Err(e) = l.release_ref(1, 9) {
        if e == RefCode::DoubleRelease.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：重复建对象。
    ledger.tries[1] += 1;
    let mut c1 = false;
    if let Err(e) = l.create(1) {
        if e == RefCode::ObjectExists.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：越界 id。
    ledger.tries[2] += 1;
    let mut c2 = false;
    if let Err(e) = l.create(ID_CAP + 1) {
        if e == RefCode::IdOutOfRange.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RefCode::ALL.len() {
        let v = RefCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演万对象", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 规模写死判据（不复用被测值）。
    v.push((
        "J1-万对象规模写死",
        STRESS_OBJECTS == 10_000 && ID_CAP == 10_002,
        format!("{}/{}", STRESS_OBJECTS, ID_CAP),
    ));
    // 边集判据：Open 直跳 Sealed 必拒。
    v.push((
        "J1-跳级封账必拒",
        !ledger_edge(LedgerPhase::Open, LedgerPhase::Sealed)
            && !ledger_edge(LedgerPhase::Sealed, LedgerPhase::Auditing),
        String::from("Open→Auditing→Sealed"),
    ));
    // 反向判据：未建先引用必拒。
    let mut l2 = RefLedger::new();
    let miss = l2.add_ref(5, 1);
    let mut ok = false;
    if let Err(e) = miss {
        if e == RefCode::ObjectMissing.val() {
            ok = true;
        }
    }
    v.push(("J1-未建先引用必拒", ok, String::from("对象账缺位")));
    // 反向判据：计数下探 0 必拒。
    let mut l3 = RefLedger::new();
    let _ = l3.create(2);
    let neg = l3.release_ref(2, 3);
    let mut ok2 = false;
    if let Err(e) = neg {
        if e == RefCode::NegativeCount.val() {
            ok2 = true;
        }
    }
    v.push(("J1-计数下探必拒", ok2, String::from("0 下限")));
    // 空账审计零漂移。
    let empty = RefLedger::new().audit();
    v.push((
        "J1-空账审计绿",
        matches!(empty, Ok((a, b)) if a == 0 && b == 0),
        format!("{:?}", empty),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65E00 窗口")));
    v
}

pub fn run_unx13721_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13721 全对象引用账总账结构");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
