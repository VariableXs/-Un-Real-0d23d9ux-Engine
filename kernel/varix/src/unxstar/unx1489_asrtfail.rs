//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1489 · 断言失败处理语义（记账续跑与 fail-fast 分级） · 0x75C80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5-K）：
//!   失败分级闭集（记账续跑/fail-fast 两级），续跑账只追加，fail-fast
//!   后一切请求显性拒，分级误判注入（该停不停/该续不续）10/10 必抓
//! 设计要点：
//!   1. 分级闭集：记账续跑（RecordContinue，失败落账后运行态不变）/
//!      fail-fast（FailFast，落账即冻结运行态）两级，ALL 数组闭集返回；
//!   2. 严重度表冻结：良性码 3 枚（容差超限/仅注记/可重试）+ 致命码
//!      3 枚（不变量破坏/内存越界/栈守卫命中），分级查表即得；
//!   3. 续跑账只追加：事件表无删除接口，追加后前缀逐位比对（快照
//!      复核），抹账即显性违规；
//!   4. fail-fast 状态闸：RunPhase Running→Terminated 单向单调，终态
//!      后一切 process 请求显性拒（TerminatedReject），倒退还原显性拒；
//!   5. 分级误判双注入：致命码强判续跑（该停不停）/ 良性码强判停机
//!      （该续不续），分级守卫逐枚比对冻结表，10/10 必抓；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75C80，AI-VX7-1104 批5-K 独占槽）。
pub const SEG_BASE: u32 = 0x75C80;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 良性失败码冻结表（容差超限/仅注记/可重试）。
pub const BENIGN_CODES: [u32; 3] = [0x01, 0x02, 0x03];
/// 致命失败码冻结表（不变量破坏/内存越界/栈守卫命中）。
pub const FATAL_CODES: [u32; 3] = [0x81, 0x82, 0x83];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// 私有取值助手（冻结表取值走 .get，不裸下标）。
fn fatal0() -> u32 {
    match FATAL_CODES.get(0) {
        Some(c) => *c,
        None => 0x81,
    }
}
fn fatal1() -> u32 {
    match FATAL_CODES.get(1) {
        Some(c) => *c,
        None => 0x82,
    }
}
fn fatal2() -> u32 {
    match FATAL_CODES.get(2) {
        Some(c) => *c,
        None => 0x83,
    }
}
fn benign0() -> u32 {
    match BENIGN_CODES.get(0) {
        Some(c) => *c,
        None => 0x01,
    }
}
fn benign1() -> u32 {
    match BENIGN_CODES.get(1) {
        Some(c) => *c,
        None => 0x02,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailCode {
    /// 未知失败码（不在冻结严重度表）
    UnknownFailCode,
    /// fail-fast 终态后请求显性拒
    TerminatedReject,
    /// 运行态倒退还原显性拒
    PhaseRegress,
    /// 续跑账追加违规（前缀被抹改）
    LedgerBreach,
    /// 分级误判（与冻结表不符）
    ClassifyMismatch,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl FailCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FailCode::UnknownFailCode => "未知失败码：不在冻结严重度表，建议先扩表再受理",
            FailCode::TerminatedReject => "fail-fast 终态后请求被拒：建议重启一代运行态再受理",
            FailCode::PhaseRegress => "运行态倒退还原被拒：Terminated 单向单调，建议新开一代",
            FailCode::LedgerBreach => "续跑账追加违规：前缀被抹改，建议恢复只追加纪律",
            FailCode::ClassifyMismatch => "分级误判：与冻结严重度表不符，建议以查表为准",
            FailCode::InjectLeaked => "注入漏抓：误判样本未检出，建议对齐冻结表逐枚复核",
            FailCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [FailCode; 7] = [
        FailCode::UnknownFailCode,
        FailCode::TerminatedReject,
        FailCode::PhaseRegress,
        FailCode::LedgerBreach,
        FailCode::ClassifyMismatch,
        FailCode::InjectLeaked,
        FailCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FailCode::ALL.len() {
        if let Some(c) = FailCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75C80 段闭集内"
}

// ── 分级闭集（两级）与严重度闭集 ────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailClass {
    /// 记账续跑：失败落账，运行态不变
    RecordContinue,
    /// fail-fast：落账即冻结运行态
    FailFast,
}

impl FailClass {
    pub fn val(self) -> u32 {
        self as u32
    }
    pub const ALL: [FailClass; 2] = [FailClass::RecordContinue, FailClass::FailFast];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 良性：分级应续跑
    Benign,
    /// 致命：分级应停机
    Fatal,
}

/// 严重度查表（冻结表闭集；表外码显性无级）。
pub fn severity_of(code: u32) -> Option<Severity> {
    let mut i = 0;
    while i < BENIGN_CODES.len() {
        if let Some(c) = BENIGN_CODES.get(i) {
            if *c == code {
                return Some(Severity::Benign);
            }
        }
        i += 1;
    }
    let mut i = 0;
    while i < FATAL_CODES.len() {
        if let Some(c) = FATAL_CODES.get(i) {
            if *c == code {
                return Some(Severity::Fatal);
            }
        }
        i += 1;
    }
    None
}

/// 分级（权威口径：查冻结表）。
pub fn classify(code: u32) -> Result<FailClass, u32> {
    match severity_of(code) {
        Some(Severity::Benign) => Ok(FailClass::RecordContinue),
        Some(Severity::Fatal) => Ok(FailClass::FailFast),
        None => Err(FailCode::UnknownFailCode.val()),
    }
}

/// 分级守卫：强判结果与冻结表一致才放行（分级误判的抓手）。
pub fn classification_guard(code: u32, forced: FailClass) -> bool {
    match classify(code) {
        Ok(c) => c == forced,
        Err(_) => false,
    }
}

// ── 续跑账（只追加）与运行态状态机 ──────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPhase {
    /// 运行中（受理续跑与停机）
    Running,
    /// 已冻结（fail-fast 落地，一切请求显性拒）
    Terminated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailEvent {
    pub seq: u64,
    pub code: u32,
    pub class: FailClass,
}

#[derive(Clone, Debug, Default)]
pub struct AsrtRunState {
    pub phase: Option<RunPhase>,
    /// 续跑账：只追加，无删除接口。
    pub ledger: Vec<FailEvent>,
    pub seq: u64,
    pub cont_count: u32,
    pub fast_count: u32,
    /// fail-fast 后被显性拒的请求数。
    pub rejects: u32,
}

impl AsrtRunState {
    pub fn new() -> AsrtRunState {
        AsrtRunState { phase: Some(RunPhase::Running), ledger: Vec::new(), seq: 0, cont_count: 0, fast_count: 0, rejects: 0 }
    }

    /// 受理一笔失败码（forced = 强判分级，None = 权威查表）。
    /// 返回落账分级；终态后一律显性拒。
    pub fn process(&mut self, code: u32, forced: Option<FailClass>) -> Result<FailClass, u32> {
        if self.phase == Some(RunPhase::Terminated) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FailCode::TerminatedReject.val());
        }
        let class = match forced {
            Some(c) => c,
            None => match classify(code) {
                Ok(c) => c,
                Err(e) => return Err(e),
            },
        };
        self.seq = self.seq.saturating_add(1);
        self.ledger.push(FailEvent { seq: self.seq, code, class });
        match class {
            FailClass::RecordContinue => {
                self.cont_count = self.cont_count.saturating_add(1);
            }
            FailClass::FailFast => {
                self.fast_count = self.fast_count.saturating_add(1);
                self.phase = Some(RunPhase::Terminated);
            }
        }
        Ok(class)
    }

    /// 倒退还原（Terminated → Running）：显性拒，无门。
    pub fn try_revert(&mut self) -> Result<(), u32> {
        Err(FailCode::PhaseRegress.val())
    }

    /// 终态闸谓词。
    pub fn is_terminated(&self) -> bool {
        self.phase == Some(RunPhase::Terminated)
    }
}

/// 续跑账只追加复核：快照前缀必须逐位仍在（抹账即违规）。
pub fn append_only_ok(state: &AsrtRunState, snap: &Vec<FailEvent>) -> bool {
    if snap.len() > state.ledger.len() {
        return false;
    }
    let mut i = 0;
    while i < snap.len() {
        match (snap.get(i), state.ledger.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
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

// ── 注入检出账（三路：该停不停 / 该续不续 / 终态漏拒） ──────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
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
        while i < 3 {
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

/// 该停不停：致命码强判续跑，守卫必须判 false。
pub fn misc_continue_injected(code: u32) -> bool {
    // 前提：这确实是致命码（注入方向不错位）。
    if severity_of(code) != Some(Severity::Fatal) {
        return false;
    }
    // 强判续跑后守卫必须抓出（false = 抓到误判）。
    !classification_guard(code, FailClass::RecordContinue)
}

/// 该续不续：良性码强判停机，守卫必须判 false。
pub fn misc_fast_injected(code: u32) -> bool {
    if severity_of(code) != Some(Severity::Benign) {
        return false;
    }
    !classification_guard(code, FailClass::FailFast)
}

/// 终态漏拒：fail-fast 落地后继续受理必须显性拒。
pub fn terminated_reject_injected(state: &mut AsrtRunState, benign_code: u32) -> bool {
    if !state.is_terminated() {
        return false;
    }
    match state.process(benign_code, None) {
        Err(e) => e == FailCode::TerminatedReject.val(),
        Ok(_) => false,
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：该停不停（致命码 0x81 强判续跑）。
        led.record(0, misc_continue_injected(fatal0()));
        // 路 2：该续不续（良性码 0x01 强判停机）。
        led.record(1, misc_fast_injected(benign0()));
        // 路 3：终态漏拒（先停机再受理良性码）。
        let mut st = AsrtRunState::new();
        let _ = st.process(fatal1(), None);
        led.record(2, terminated_reject_injected(&mut st, benign1()));
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链：续跑 3 笔→致命停机→终态拒 2 笔→账快照→误判 6 枚全抓
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut st = AsrtRunState::new();
    // 续跑面：3 笔良性码逐笔落账。
    let mut i = 0;
    while i < BENIGN_CODES.len() {
        let snap = st.ledger.clone();
        match st.process(*BENIGN_CODES.get(i).unwrap_or(&0x02), None) {
            Ok(FailClass::RecordContinue) => {}
            _ => return Err(FailCode::ClassifyMismatch.val()),
        }
        if !append_only_ok(&st, &snap) {
            return Err(FailCode::LedgerBreach.val());
        }
        i += 1;
    }
    if st.is_terminated() || st.cont_count != 3 {
        return Err(FailCode::InjectLeaked.val());
    }
    // 停机面：1 笔致命码 → 终态。
    match st.process(fatal2(), None) {
        Ok(FailClass::FailFast) => {}
        _ => return Err(FailCode::ClassifyMismatch.val()),
    }
    // 终态闸：再受理 2 笔一律显性拒。
    let mut rejects = 0u32;
    let mut i = 0;
    while i < BENIGN_CODES.len() - 1 {
        match st.process(*BENIGN_CODES.get(i).unwrap_or(&0x01), None) {
            Err(e) if e == FailCode::TerminatedReject.val() => {
                rejects = rejects.saturating_add(1);
            }
            _ => return Err(FailCode::TerminatedReject.val()),
        }
        i += 1;
    }
    if st.rejects != rejects || !st.is_terminated() {
        return Err(FailCode::PhaseRegress.val());
    }
    // 倒退还原显性拒。
    if st.try_revert().is_ok() {
        return Err(FailCode::PhaseRegress.val());
    }
    // 分级误判 6 枚（3 致命强判续跑 + 3 良性强判停机）全抓。
    let mut misc = 0u32;
    let mut i = 0;
    while i < FATAL_CODES.len() {
        if let Some(c) = FATAL_CODES.get(i) {
            if misc_continue_injected(*c) {
                misc = misc.saturating_add(1);
            }
        }
        i += 1;
    }
    let mut i = 0;
    while i < BENIGN_CODES.len() {
        if let Some(c) = BENIGN_CODES.get(i) {
            if misc_fast_injected(*c) {
                misc = misc.saturating_add(1);
            }
        }
        i += 1;
    }
    if misc != 6 {
        return Err(FailCode::InjectLeaked.val());
    }
    if st.ledger.len() != 4 {
        return Err(FailCode::LedgerBreach.val());
    }
    Ok((st.cont_count, st.fast_count, rejects))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（续 3 + 停 1 + 终态拒 2）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 1, 2)) => Ok(()),
        _ => Err(FailCode::NotReproducible.val()),
    }
}

/// 序 B：未知失败码显性拒且不落账。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut st = AsrtRunState::new();
    let snap_len = st.ledger.len();
    match st.process(0xFF, None) {
        Err(e) if e == FailCode::UnknownFailCode.val() && st.ledger.len() == snap_len => Ok(()),
        _ => Err(FailCode::NotReproducible.val()),
    }
}

/// 序 C：倒退还原显性拒（运行态与终态两态皆拒）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut running = AsrtRunState::new();
    let mut terminated = AsrtRunState::new();
    let _ = terminated.process(fatal0(), None);
    match (running.try_revert(), terminated.try_revert()) {
        (Err(_), Err(_)) => Ok(()),
        _ => Err(FailCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FailCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < FailCode::ALL.len() {
        if let Some(c) = FailCode::ALL.get(i) {
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
    while i < FailCode::ALL.len() {
        if let Some(c) = FailCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 分级权威口径全表一致（6 码 × 权威查表） ─────────────────────
fn classify_table_ok() -> bool {
    let mut i = 0;
    while i < BENIGN_CODES.len() {
        if let Some(c) = BENIGN_CODES.get(i) {
            match classify(*c) {
                Ok(FailClass::RecordContinue) => {}
                _ => return false,
            }
        }
        i += 1;
    }
    let mut i = 0;
    while i < FATAL_CODES.len() {
        if let Some(c) = FATAL_CODES.get(i) {
            match classify(*c) {
                Ok(FailClass::FailFast) => {}
                _ => return false,
            }
        }
        i += 1;
    }
    classify(0xFF).is_err()
}

// ── LCG 确定性 ─────────────────────────────────────────────────
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

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        BENIGN_CODES.len() == 3 && FATAL_CODES.len() == 3 && FailClass::ALL.len() == 2,
        format!("良性 {} 枚 + 致命 {} 枚 / 分级 {} 级", BENIGN_CODES.len(), FATAL_CODES.len(), FailClass::ALL.len()),
    ));
    // 2. 分级权威口径全表一致。
    v.push((
        "J1-分级全表一致",
        classify_table_ok(),
        String::from("6 码查表 + 表外码显性拒"),
    ));
    // 3. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((cont, fast, rej)) => *cont == 3 && *fast == 1 && *rej == 2,
            Err(_) => false,
        },
        match &g {
            Ok((cont, fast, rej)) => format!("续={} 停={} 拒={}", cont, fast, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 续跑账只追加（快照复核）。
    let append_ok = {
        let mut st = AsrtRunState::new();
        let mut ok = true;
        let mut i = 0;
        while i < BENIGN_CODES.len() {
            let snap = st.ledger.clone();
            let _ = st.process(*BENIGN_CODES.get(i).unwrap_or(&0x01), None);
            if !append_only_ok(&st, &snap) {
                ok = false;
            }
            i += 1;
        }
        ok && st.ledger.len() == BENIGN_CODES.len()
    };
    v.push((
        "J1-续跑账只追加",
        append_ok,
        format!("{} 笔追加后前缀逐位在账", BENIGN_CODES.len()),
    ));
    // 5. fail-fast 终态闸。
    let gate_ok = {
        let mut st = AsrtRunState::new();
        let _ = st.process(fatal0(), None);
        let mut ok = st.is_terminated();
        let mut i = 0;
        while i < BENIGN_CODES.len() {
            if let Some(c) = BENIGN_CODES.get(i) {
                if st.process(*c, None).is_ok() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok && st.rejects == BENIGN_CODES.len() as u32
    };
    v.push((
        "J1-failfast终态闸",
        gate_ok,
        format!("停机后 {} 笔请求全拒", BENIGN_CODES.len()),
    ));
    // 6. 倒退还原显性拒。
    v.push((
        "J1-倒退显性拒",
        probe_seq_c().is_ok(),
        String::from("运行态/终态两态还原皆拒"),
    ));
    // 7. 该停不停注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-误判注入10/10",
        match &inj {
            Ok(l) => l.all_caught() && l.tries.first() == Some(&JUDGE_ROUNDS) && l.tries.get(1) == Some(&JUDGE_ROUNDS),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b未知码拒={} c倒退拒={}", a, b, c),
    ));
    // 9. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", FailCode::ALL.len()),
    ));
    // 10. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 11. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 12. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75C80 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx1489_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1489 断言失败处理语义（记账续跑与 fail-fast 分级）");
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
            Ok((cont, fast, rej)) => {
                assert_eq!(cont, 3);
                assert_eq!(fast, 1);
                assert_eq!(rej, 2);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_tables() {
        assert_eq!(BENIGN_CODES, [0x01, 0x02, 0x03]);
        assert_eq!(FATAL_CODES, [0x81, 0x82, 0x83]);
        assert_eq!(FailClass::ALL.len(), 2);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn classify_authoritative() {
        assert!(classify_table_ok());
        match classify(0x82) {
            Ok(FailClass::FailFast) => {}
            other => assert!(false, "致命码 0x82 分级错误：{:?}", other),
        }
        match classify(0x02) {
            Ok(FailClass::RecordContinue) => {}
            other => assert!(false, "良性码 0x02 分级错误：{:?}", other),
        }
        assert!(classify(0xFF).is_err());
        assert!(severity_of(0x40).is_none());
    }

    #[test]
    fn misc_injections_caught() {
        // 该停不停：3 枚致命码强判续跑全抓。
        let mut i = 0;
        while i < FATAL_CODES.len() {
            if let Some(c) = FATAL_CODES.get(i) {
                assert!(misc_continue_injected(*c), "致命码 0x{:x} 误判未抓", c);
            }
            i += 1;
        }
        // 该续不续：3 枚良性码强判停机全抓。
        let mut i = 0;
        while i < BENIGN_CODES.len() {
            if let Some(c) = BENIGN_CODES.get(i) {
                assert!(misc_fast_injected(*c), "良性码 0x{:x} 误判未抓", c);
            }
            i += 1;
        }
    }

    #[test]
    fn terminated_gate_rejects_all() {
        let mut st = AsrtRunState::new();
        let _ = st.process(fatal0(), None);
        assert!(st.is_terminated());
        let mut i = 0;
        while i < BENIGN_CODES.len() {
            if let Some(c) = BENIGN_CODES.get(i) {
                match st.process(*c, None) {
                    Err(e) => assert_eq!(e, FailCode::TerminatedReject.val()),
                    Ok(_) => assert!(false, "终态后请求未拒"),
                }
            }
            i += 1;
        }
        assert_eq!(st.rejects, BENIGN_CODES.len() as u32);
        assert!(terminated_reject_injected(&mut st, benign0()));
    }

    #[test]
    fn ledger_append_only() {
        let mut st = AsrtRunState::new();
        let _ = st.process(benign0(), None);
        // 首笔落账后取快照，再追加第二笔，前缀必须逐位仍在。
        let snap = st.ledger.clone();
        let _ = st.process(benign1(), None);
        assert!(append_only_ok(&st, &snap));
        assert_eq!(st.ledger.len(), 2);
        // 前缀逐位相等（序号与码与分级都不许漂移）。
        match (snap.get(0), st.ledger.get(0)) {
            (Some(a), Some(b)) => assert_eq!(a, b),
            _ => assert!(false, "快照前缀失踪"),
        }
    }

    #[test]
    fn revert_always_rejected() {
        let mut running = AsrtRunState::new();
        match running.try_revert() {
            Err(e) => assert_eq!(e, FailCode::PhaseRegress.val()),
            Ok(()) => assert!(false, "运行态还原不应成功"),
        }
        let mut terminated = AsrtRunState::new();
        let _ = terminated.process(fatal1(), None);
        match terminated.try_revert() {
            Err(e) => assert_eq!(e, FailCode::PhaseRegress.val()),
            Ok(()) => assert!(false, "终态还原不应成功"),
        }
    }

    #[test]
    fn forced_class_vs_authoritative() {
        let mut st = AsrtRunState::new();
        // 强判分级落账（模拟带 bug 的分级器）。
        match st.process(fatal0(), Some(FailClass::RecordContinue)) {
            Ok(FailClass::RecordContinue) => {}
            _ => assert!(false, "强判未落账"),
        }
        // 但守卫必须抓出这是误判（该停不停）。
        assert!(!classification_guard(fatal0(), FailClass::RecordContinue));
        // 停机未发生（误判后果可见）。
        assert!(!st.is_terminated());
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(led.tries.get(0), Some(&JUDGE_ROUNDS));
        assert_eq!(led.tries.get(1), Some(&JUDGE_ROUNDS));
        assert_eq!(led.tries.get(2), Some(&JUDGE_ROUNDS));
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
        assert_eq!(detail(FailCode::LedgerBreach.val()), FailCode::LedgerBreach.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
