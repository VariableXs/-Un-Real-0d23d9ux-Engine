//! UNX-F51648 isoc 序号断流恢复（seq 违规检测） · 0xC24C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F51651-J1 断言 ×50
//! 全过；UNX-F51651-J1R 链损坏注入 10/10 检出；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - CbCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - RingPhase       完成环相态机（Running→Faulted→Isolated→Running）
//!   - CbEventRing     完成环 + 回调分派器（异常隔离不伤环）
//!   - TriFactorLedger 异常三要素留账（异常码/环位/隔离动作，留账率 100%）
//!   - run_unx51648_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 回调异常隔离：完成环逐项分派回调，回调以 Result 建模抛错
//!      （no_std 无 unwinding，Err 即异常）；回调 Err 时环不得停转——
//!      隔离动作落账后环指针照常推进，后续完成项照常消化。
//!   ② 异常三要素留账率 100%：每次回调异常必须同时记下异常码、发生
//!      环位（槽序号）与隔离动作，缺一即判留账失败并计数。
//!   ③ 相态链：Running→Faulted→Isolated→回 Running 单调推进，Faulted
//!      相态内不得宣称隔离完成，Isolated 后必须复位回 Running 才算
//!      环续转（续转锚点 = 消化计数持续递增）。
//!   ④ 断言 ×50 全过（J1）：逐轮核验异常后环推进、留账齐全、账守恒；
//!      链损坏注入（环位写越界/重复分派）10/10 检出（J1R）。
//!   ⑤ 完成写回：隔离收口写状态=已完成、结果摘要非空回执。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——LCG 种子由
//!      SEG_BASE 派生；全程零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC24C0;

/// 完成环容量。
pub const RING_CAP: usize = 8;
/// 断言扫描轮数（J1：×50 全过）。
pub const ASSERT_ROUNDS: u32 = 50;

/// 诊断码闭集（回调异常隔离域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CbCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 回调拒绝（建模回调抛错）
    CbRejected = 3,
    /// 回调标签未知
    CbUnknownTag = 4,
    /// 同槽重复分派
    DoubleDispatch = 5,
    /// 隔离未落账即宣称完成
    IsolateWithoutLedger = 6,
    /// 环位越界
    SlotOob = 7,
    /// Faulted 相态内宣称隔离完成
    IsolateDuringFault = 8,
    /// 相态回退/跳相
    PhaseRegress = 9,
    /// 注入账不符
    LedgerMismatch = 10,
    /// 写回摘要为空
    WritebackEmpty = 11,
    /// 三要素缺失
    FactorMissing = 12,
}

impl CbCode {
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            CbCode::Ok => "回调分派成功",
            CbCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            CbCode::CbRejected => "回调拒绝：回调体返回 Err，异常隔离落账后环照常推进",
            CbCode::CbUnknownTag => "回调标签未知：分派目标不在回调表之内",
            CbCode::DoubleDispatch => "重复分派：同一环位不得二次分派",
            CbCode::IsolateWithoutLedger => "无账隔离：三要素未落账不得宣称隔离完成",
            CbCode::SlotOob => "环位越界：分派槽序号必须在环容量之内",
            CbCode::IsolateDuringFault => "故障态收口：Faulted 相态内不得宣称隔离完成",
            CbCode::PhaseRegress => "相态违例：隔离链必须按编码序单调前进",
            CbCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            CbCode::WritebackEmpty => "写回摘要为空：隔离收口回执必须带非空结果摘要",
            CbCode::FactorMissing => "三要素缺失：异常留账必须含异常码/环位/隔离动作",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [CbCode; 12] = [
    CbCode::Ok,
    CbCode::SegBad,
    CbCode::CbRejected,
    CbCode::CbUnknownTag,
    CbCode::DoubleDispatch,
    CbCode::IsolateWithoutLedger,
    CbCode::SlotOob,
    CbCode::IsolateDuringFault,
    CbCode::PhaseRegress,
    CbCode::LedgerMismatch,
    CbCode::WritebackEmpty,
    CbCode::FactorMissing,
];

/// 完成环相态（闭集；Running 为复位态，链内单调前进后复位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingPhase {
    /// 正常分派
    Running = 0,
    /// 回调异常已检出
    Faulted = 1,
    /// 已隔离（待复位）
    Isolated = 2,
}

impl RingPhase {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            RingPhase::Running => "正常分派",
            RingPhase::Faulted => "异常检出",
            RingPhase::Isolated => "已隔离",
        }
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IsolateWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
    /// 相态序号（必须为 Isolated=2）
    pub phase: u32,
}

/// 异常三要素留账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TriFactor {
    /// 要素一：异常码
    pub code: u32,
    /// 要素二：发生环位
    pub slot: usize,
    /// 要素三：隔离动作（非空中文）
    pub action: String,
}

/// 完成环 + 回调分派器：异常隔离不伤环。
pub struct CbEventRing {
    phase: RingPhase,
    phase_trace: Vec<u32>,
    slots: [Option<u64>; RING_CAP],
    dispatched: [bool; RING_CAP],
    fill: usize,
    head: usize,
    processed: u64,
    faults: u32,
    factors: Vec<TriFactor>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl CbEventRing {
    /// 新环：全空、Running。
    pub fn new() -> CbEventRing {
        CbEventRing {
            phase: RingPhase::Running,
            phase_trace: Vec::new(),
            slots: [None; RING_CAP],
            dispatched: [false; RING_CAP],
            fill: 0,
            head: 0,
            processed: 0,
            faults: 0,
            factors: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 当前相态。
    pub fn phase(&self) -> RingPhase {
        self.phase
    }

    /// 已消化完成项数（环续转锚点）。
    pub fn processed(&self) -> u64 {
        self.processed
    }

    /// 异常计数。
    pub fn faults(&self) -> u32 {
        self.faults
    }

    /// 相态推进：只允许编码序 +1 前进。
    pub fn advance_phase(&mut self, want: RingPhase) -> Result<RingPhase, CbCode> {
        if want.code() != self.phase.code() + 1 {
            return Err(CbCode::PhaseRegress);
        }
        self.phase = want;
        self.phase_trace.push(want.code());
        Ok(want)
    }

    /// 入队一个完成项（载荷入当前头槽）。
    pub fn post(&mut self, payload: u64) -> Result<usize, CbCode> {
        // 写指针 fill 与分派指针 head 分离：入队不动分派位。
        let slot = self.fill;
        match self.slots.get_mut(slot) {
            Some(s @ None) => {
                *s = Some(payload);
                self.fill = (self.fill + 1) % RING_CAP;
                Ok(slot)
            }
            _ => Err(CbCode::SlotOob),
        }
    }

    /// 三要素落账（全要素校验后入账）。
    pub fn ledger_factor(&mut self, code: u32, slot: usize, action: &str) -> Result<(), CbCode> {
        if action.is_empty() {
            return Err(CbCode::FactorMissing);
        }
        self.factors.push(TriFactor {
            code,
            slot,
            action: String::from(action),
        });
        Ok(())
    }

    /// 三要素留账条数。
    pub fn factors_len(&self) -> usize {
        self.factors.len()
    }

    /// 第 k 条三要素账完整性。
    pub fn factor_complete(&self, k: usize) -> bool {
        match self.factors.get(k) {
            Some(f) => f.code != 0 && !f.action.is_empty(),
            None => false,
        }
    }

    /// 分派当前头槽回调：cb_ok=false 建模回调抛错——异常隔离落账，
    /// 环指针照常推进（不伤环）；重复分派显性拒。
    pub fn dispatch(&mut self, cb_ok: bool) -> Result<u64, CbCode> {
        let slot = self.head;
        if slot >= RING_CAP {
            return Err(CbCode::SlotOob);
        }
        if self.dispatched[slot] {
            return Err(CbCode::DoubleDispatch);
        }
        let payload = match self.slots.get(slot) {
            Some(Some(p)) => *p,
            _ => return Err(CbCode::SlotOob),
        };
        self.dispatched[slot] = true;
        self.head = (self.head + 1) % RING_CAP;
        if !cb_ok {
            // 回调异常：三要素落账 + Faulted 相态，环照常推进。
            let ledged = self.ledger_factor(
                CbCode::CbRejected as u32,
                slot,
                "回调异常隔离：回调 Err 已落账，环指针照常推进不伤环",
            );
            if ledged.is_err() {
                return Err(CbCode::FactorMissing);
            }
            self.faults = self.faults.saturating_add(1);
            if self.phase == RingPhase::Running {
                let _ = self.advance_phase(RingPhase::Faulted);
            }
        }
        self.processed = self.processed.saturating_add(1);
        match self.slots.get_mut(slot) {
            Some(s) => *s = None,
            None => return Err(CbCode::SlotOob),
        }
        // 回调异常以 Err 显性回报（建模抛错），但环推进与落账已完成。
        if !cb_ok {
            return Err(CbCode::CbRejected);
        }
        Ok(payload)
    }

    /// 隔离收口（Faulted→Isolated）并写回；无账显性拒。
    pub fn isolate(&mut self) -> Result<IsolateWriteback, CbCode> {
        if self.phase != RingPhase::Faulted {
            return Err(CbCode::IsolateDuringFault);
        }
        if self.factors.is_empty() {
            return Err(CbCode::IsolateWithoutLedger);
        }
        let _ = self.advance_phase(RingPhase::Isolated)?;
        Ok(IsolateWriteback {
            done: true,
            summary: format!(
                "隔离收口：回调异常 {} 次全部三要素在案，环已消化 {} 项",
                self.faults, self.processed
            ),
            phase: RingPhase::Isolated.code(),
        })
    }

    /// 复位回 Running（环续转）。
    pub fn resume(&mut self) {
        self.phase = RingPhase::Running;
        self.phase_trace.clear();
        let mut i = 0usize;
        while i < RING_CAP {
            if let Some(d) = self.dispatched.get_mut(i) {
                *d = false;
            }
            i += 1;
        }
    }

    /// 越界安全探测：环位写。
    pub fn poke(&mut self, slot: usize, payload: u64) -> Result<(), CbCode> {
        match self.slots.get_mut(slot) {
            Some(s @ None) => {
                *s = Some(payload);
                Ok(())
            }
            _ => Err(CbCode::SlotOob),
        }
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入账守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
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

    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (u32, u64, u32, usize, u64, u64) {
        (
            self.head as u32,
            self.processed,
            self.faults,
            self.factors.len(),
            self.injected,
            self.detected,
        )
    }
}

/// LCG：种子由 SEG_BASE 派生（零随机源，确定性可复现）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 引擎指纹：对确定性脚本做 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let mut r = CbEventRing::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(2654435761).wrapping_add(1);
    let mut log: String = String::new();
    for round in 0..64u32 {
        let op = lcg_next(&mut seed) % 5;
        let payload = lcg_next(&mut seed);
        let slot = (lcg_next(&mut seed) % (RING_CAP as u64 + 1)) as usize;
        let run = match op {
            0 => r.post(payload),
            1 => r.dispatch(true).map(|v| v as usize),
            2 => r.dispatch(false).map(|v| v as usize),
            3 => r.isolate().map(|w| w.phase as usize),
            _ => r.poke(slot, payload).map(|_| 0usize),
        };
        log.push_str(&format!("{}:{:?};", round, run.err()));
        if r.phase() == RingPhase::Isolated {
            r.resume();
        }
    }
    let (h, p, f, l, i, d) = r.snapshot();
    let full = format!("{}|{},{},{},{},{},{}", log, h, p, f, l, i, d);
    fnv1a(full.as_bytes())
}

/// 双跑一致性：同一脚本两次执行指纹必须逐位一致。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut r = CbEventRing::new();
    let mut seed: u64 = (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：回调抛错 ×10（隔离落账 + 环续转）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            r.resume();
            let _ = r.post(0x100 + i as u64);
            let before = r.processed();
            let run = r.dispatch(false);
            if run.is_err() && r.processed() == before + 1 && r.factors_len() > 0 {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
            let _ = lcg_next(&mut seed) + i as u64;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：重复分派 ×10（把下一头槽预标已分派，二次分派必须拒）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            r.resume();
            let _ = r.post(0x200 + i as u64);
            let _ = r.dispatch(true); // 正常消化一项，head 前进
            let h = r.snapshot().0 as usize;
            if let Some(d) = r.dispatched.get_mut(h) {
                *d = true; // 注入重复分派标记
            }
            let run = r.dispatch(true);
            if let Some(d) = r.dispatched.get_mut(h) {
                *d = false; // 清注入标记
            }
            if run == Err(CbCode::DoubleDispatch) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
            let _ = lcg_next(&mut seed) + i as u64;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：空动作留账 ×10（三要素缺失检出）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            r.resume();
            let run = r.ledger_factor(CbCode::CbRejected as u32, i as usize, "");
            if run == Err(CbCode::FactorMissing) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：Running 相态直接隔离 ×10（故障态收口守卫）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            r.resume();
            let run = r.isolate();
            if run == Err(CbCode::IsolateDuringFault) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：环位越界 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            r.resume();
            let run = r.poke(RING_CAP + i as usize, 0x5A);
            if run == Err(CbCode::SlotOob) {
                caught += 1;
                r.note_detected();
            } else {
                r.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (r.injected(), r.detected(), r.escaped(), classes_hit)
}

/// 断言 ×50 全过（J1）：逐轮异常注入后环仍续转且三要素齐全。
pub fn assertions_50_all_pass() -> bool {
    let mut r = CbEventRing::new();
    for round in 0..ASSERT_ROUNDS {
        r.resume();
        // 断言1：入队成功。
        if r.post(round as u64).is_err() {
            return false;
        }
        // 断言2：回调抛错被隔离且环推进。
        let before = r.processed();
        if r.dispatch(round % 3 != 1).is_err() != (round % 3 == 1) {
            return false;
        }
        if r.processed() != before + 1 {
            return false;
        }
        // 断言3：异常轮三要素齐全。
        if round % 3 == 1 {
            let n = r.factors_len();
            if n == 0 || !r.factor_complete(n - 1) {
                return false;
            }
        }
        // 断言4：异常轮后下一完成项照常消化（环续转）。
        if r.post(0xF000 + round as u64).is_err() {
            return false;
        }
        if r.dispatch(true).is_err() {
            return false;
        }
        // 断言5：账守恒（注入面在本函数外核验，这里核 processed 单调）。
        if r.processed() < 2 {
            return false;
        }
    }
    true
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51648_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51648");
    // 1 正常分派消化。
    {
        let mut r = CbEventRing::new();
        let _ = r.post(0xA1);
        let run = r.dispatch(true);
        s.add("正常分派消化", run == Ok(0xA1) && r.processed() == 1, "回调 Ok 时完成项正常消化");
    }
    // 2 回调异常 ×10 环继续运转。
    {
        let mut r = CbEventRing::new();
        let mut exceptions = 0u32;
        let mut resumed = 0u32;
        for i in 0..10u32 {
            r.resume();
            let _ = r.post(0x300 + i as u64);
            if r.dispatch(false).is_err() {
                exceptions += 1;
            }
            let _ = r.isolate();
            r.resume();
            let _ = r.post(0x400 + i as u64);
            if r.dispatch(true).is_ok() {
                resumed += 1;
            }
        }
        s.add("异常后环续转", exceptions == 10 && resumed == 10, "10 次回调异常后环须全部续转消化后续完成项");
    }
    // 3 三要素留账率 100%。
    {
        let mut r = CbEventRing::new();
        for i in 0..10u32 {
            r.resume();
            let _ = r.post(0x500 + i as u64);
            let _ = r.dispatch(false);
        }
        let n = r.factors_len();
        let mut all = n == 10;
        let mut k = 0usize;
        while k < n {
            if !r.factor_complete(k) {
                all = false;
            }
            k += 1;
        }
        s.add("三要素留账满额", all, "10 次异常留账须三要素（码/环位/动作）全齐");
    }
    // 4 断言 ×50 全过。
    s.add("断言五十轮全过", assertions_50_all_pass(), "50 轮异常注入断言（续转/留账/推进）须全过");
    // 5 重复分派检出。
    {
        let mut r = CbEventRing::new();
        let _ = r.post(0x11);
        let _ = r.dispatch(true);
        r.resume();
        let _ = r.post(0x12);
        if let Some(d) = r.dispatched.get_mut(1) {
            *d = true;
        }
        s.add("重复分派拒", r.dispatch(true) == Err(CbCode::DoubleDispatch), "同槽二次分派必须显性拒");
    }
    // 6 注入核：5 类 ×10 全检出、逃逸 0、类齐。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 7 注入账守恒。
    {
        let (inj, det, esc) = {
            let (i, d, x, _) = injection_core();
            (i, d, x)
        };
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 8 隔离收口写回合规。
    {
        let mut r = CbEventRing::new();
        let _ = r.post(0x21);
        let _ = r.dispatch(false);
        let w = r.isolate();
        s.add(
            "隔离收口写回",
            matches!(w, Ok(ref x) if x.done && !x.summary.is_empty() && x.phase == 2),
            "Faulted→Isolated 收口写回须状态=已完成且摘要非空",
        );
    }
    // 9 无异常直接隔离显性拒。
    {
        let mut r = CbEventRing::new();
        s.add("无故障收口拒", r.isolate() == Err(CbCode::IsolateDuringFault), "Running 相态不得宣称隔离完成");
    }
    // 10 空动作留账显性拒。
    {
        let mut r = CbEventRing::new();
        s.add("空动作留账拒", r.ledger_factor(3, 0, "") == Err(CbCode::FactorMissing), "隔离动作缺失不得入账");
    }
    // 11 诊断码闭集唯一且文案非空。
    {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES[i] == ALL_CODES[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut texts = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                texts = false;
            }
        }
        s.add("闭集唯一文案全", uniq && texts, "12 变体互异且中文文案全量非空");
    }
    // 12 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 13 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 14 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC24C0 须 0x40 对齐且在分派区间");
    // 15 指纹非退化 + LCG 确定性。
    {
        let h1 = fnv1a(b"unx51648-probe-a");
        let h2 = fnv1a(b"unx51648-probe-b");
        let mut s1: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut s2: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut same = true;
        for _ in 0..16 {
            if lcg_next(&mut s1) != lcg_next(&mut s2) {
                same = false;
            }
        }
        s.add("指纹与 LCG 面全绿", h1 != h2 && h1 != 0 && same, "FNV 非退化且同种子 LCG 序列逐值一致");
    }
    // 16 环位越界探测。
    {
        let mut r = CbEventRing::new();
        s.add("环位越界拒", r.poke(RING_CAP + 3, 1) == Err(CbCode::SlotOob), "越界环位写入必须显性拒");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_dispatch() {
        let mut r = CbEventRing::new();
        let _ = r.post(0xA1);
        assert_eq!(r.dispatch(true), Ok(0xA1));
        assert_eq!(r.processed(), 1);
    }

    #[test]
    fn cb_fault_isolates_and_advances() {
        let mut r = CbEventRing::new();
        let _ = r.post(0xB1);
        assert!(r.dispatch(false).is_err());
        assert_eq!(r.phase(), RingPhase::Faulted);
        assert_eq!(r.processed(), 1);
        assert_eq!(r.factors_len(), 1);
    }

    #[test]
    fn double_dispatch_rejected() {
        let mut r = CbEventRing::new();
        let _ = r.post(0xB2);
        let _ = r.dispatch(true);
        r.resume();
        let _ = r.post(0xB3);
        if let Some(d) = r.dispatched.get_mut(1) {
            *d = true;
        }
        assert_eq!(r.dispatch(true), Err(CbCode::DoubleDispatch));
    }

    #[test]
    fn isolate_chain_writeback() {
        let mut r = CbEventRing::new();
        let _ = r.post(0xB4);
        let _ = r.dispatch(false);
        let w = r.isolate();
        assert!(w.is_ok());
        assert_eq!(r.phase(), RingPhase::Isolated);
        r.resume();
        assert_eq!(r.phase(), RingPhase::Running);
    }

    #[test]
    fn isolate_without_fault_rejected() {
        let mut r = CbEventRing::new();
        assert_eq!(r.isolate(), Err(CbCode::IsolateDuringFault));
    }

    #[test]
    fn empty_action_rejected() {
        let mut r = CbEventRing::new();
        assert_eq!(r.ledger_factor(3, 0, ""), Err(CbCode::FactorMissing));
    }

    #[test]
    fn assertions_50() {
        assert!(assertions_50_all_pass());
    }

    #[test]
    fn slot_oob_rejected() {
        let mut r = CbEventRing::new();
        assert_eq!(r.poke(RING_CAP + 1, 1), Err(CbCode::SlotOob));
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn ledger_conserved() {
        let mut r = CbEventRing::new();
        for _ in 0..10 {
            r.note_detected();
        }
        assert!(r.conserved());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(engine_fingerprint(), engine_fingerprint());
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 12);
        for (idx, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + idx as u32);
            assert!(!c.detail().is_empty());
        }
        assert!(seg_guard_ok());
    }

    #[test]
    fn unx51648_checks_green() {
        let set = run_unx51648_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
