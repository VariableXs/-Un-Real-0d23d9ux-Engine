//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1335 · 等价验证与 B28 压测衔接账（等价保持下的压测门禁） · 0x7750
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1335-J1 压测门禁协议（B28 压测中等价断言常驻）执行率
//!      100%，压测中等价破约即时拦截 10/10 次，门禁与 B28 资产集成
//!      断言常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 门禁协议：B28 压测会话每个 tick 必须随行等价证据（ppm），无证
//!     或破线（ppm<10^6）即拦截；门禁常驻=会话全程 tick 逐个过闸；
//!   - 即时拦截：破约 tick 当场截断会话（后续 tick 不再执行），拦截
//!     记录入账（会话号+破约 tick+观测 ppm）；
//!   - B28 资产登记：≥3 类压测资产（极端率/多核并发/长稳）挂门禁，
//!     集成断言=每资产至少一次全程过闸会话；
//!   - 会话账：会话号单调、tick 守恒（计划=已跑+被截）、过闸率 ppm；
//!   - 反向判据：无证据 tick 必拦、拔闸（关守卫）必拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x7750 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x7750;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// B28 资产宇宙（3 类挂门禁）。
pub const ASSET_COUNT: usize = 3;

/// 门禁等价线。
pub const GATE_PPM: u64 = 1_000_000;

/// 0x7750 无证 tick（证据缺省）。
pub const E_NO_EVIDENCE: u16 = SEG_BASE + 0;
/// 0x7751 破约放行（ppm<10^6 未拦）。
pub const E_BREACH_PASS: u16 = SEG_BASE + 1;
/// 0x7752 拦截漏账（截断未入账）。
pub const E_INTERCEPT_OMIT: u16 = SEG_BASE + 2;
/// 0x7753 tick 守恒破约（计划≠已跑+被截）。
pub const E_TICK_CONSERVE: u16 = SEG_BASE + 3;
/// 0x7754 会话号失序。
pub const E_SESSION_SEQ: u16 = SEG_BASE + 4;
/// 0x7755 资产未挂闸（压测未接门禁）。
pub const E_ASSET_UNGATED: u16 = SEG_BASE + 5;
/// 0x7756 拔闸得手（守卫被关闭仍放行）。
pub const E_GATE_OFF: u16 = SEG_BASE + 6;
/// 0x7757 集成断言红（资产无全程过闸会话）。
pub const E_INTEGRATE_RED: u16 = SEG_BASE + 7;
/// 0x7758 会话越界。
pub const E_SESSION_OOB: u16 = SEG_BASE + 8;
/// 0x7759 拦截不即时（破约后仍跑后续 tick）。
pub const E_NOT_IMMEDIATE: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_NO_EVIDENCE, E_BREACH_PASS, E_INTERCEPT_OMIT, E_TICK_CONSERVE,
    E_SESSION_SEQ, E_ASSET_UNGATED, E_GATE_OFF, E_INTEGRATE_RED,
    E_SESSION_OOB, E_NOT_IMMEDIATE,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_EVIDENCE => "无证 tick：证据随行制，生成侧必须逐 tick 供 ppm，查供给短路",
        E_BREACH_PASS => "破约放行：门禁只认 ppm==10^6，破线即截，禁容差",
        E_INTERCEPT_OMIT => "拦截漏账：截断必入账（会话+tick+ppm），查拦截分支漏写",
        E_TICK_CONSERVE => "tick 守恒破约：计划=已跑+被截，逐会话对账定位",
        E_SESSION_SEQ => "会话号失序：会话号单调分配，禁跳号重号",
        E_ASSET_UNGATED => "资产未挂闸：压测资产注册时必须绑门禁，先绑再跑",
        E_GATE_OFF => "拔闸得手：守卫开关只读（恒 true），关闸入口必须封死",
        E_INTEGRATE_RED => "集成断言红：每资产至少一次全程过闸会话，缺则补跑",
        E_SESSION_OOB => "会话越界：会话表界内访问，越界即拒",
        E_NOT_IMMEDIATE => "拦截不即时：破约 tick 当场截断，禁缓冲到会话尾",
        _ => "未知诊断码：核对 0x7750 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 LCG 与 B28 资产宇宙
// ═══════════════════════════════════════════════════════════════════════

/// 64 位 LCG。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 建（种子写死）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407)
                | 1,
        }
    }

    /// 下一原始值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next_u64() % m
    }
}

/// B28 压测资产（挂门禁的三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum B28Asset {
    /// 极端事件率压测。
    ExtremeRate,
    /// 多核并发压测。
    MultiCore,
    /// 长稳压测。
    LongStable,
}

impl B28Asset {
    /// 闭集。
    pub const ALL: [B28Asset; 3] =
        [B28Asset::ExtremeRate, B28Asset::MultiCore, B28Asset::LongStable];

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            B28Asset::ExtremeRate => "极端事件率",
            B28Asset::MultiCore => "多核并发",
            B28Asset::LongStable => "长稳",
        }
    }

    /// 枚举序。
    pub const fn idx(self) -> usize {
        match self {
            B28Asset::ExtremeRate => 0,
            B28Asset::MultiCore => 1,
            B28Asset::LongStable => 2,
        }
    }

    /// 从序号还原。
    pub const fn from_idx(i: usize) -> Option<B28Asset> {
        match i {
            0 => Some(B28Asset::ExtremeRate),
            1 => Some(B28Asset::MultiCore),
            2 => Some(B28Asset::LongStable),
            _ => None,
        }
    }

    /// 会话计划 tick 数。
    pub const fn plan_ticks(self) -> u64 {
        match self {
            B28Asset::ExtremeRate => 64,
            B28Asset::MultiCore => 48,
            B28Asset::LongStable => 96,
        }
    }
}

/// B28 资产登记账（挂闸后才能开跑）。
pub struct AssetGate {
    pub gated: [bool; 3],
    pub count: usize,
}

impl AssetGate {
    /// 建（全未挂闸）。
    pub fn new() -> Self {
        AssetGate { gated: [false; 3], count: 0 }
    }

    /// 挂闸（重复挂拒）。
    pub fn attach(&mut self, a: B28Asset) -> Result<(), u16> {
        let i = a.idx();
        if self.gated[i] {
            return Err(E_ASSET_UNGATED);
        }
        self.gated[i] = true;
        self.count += 1;
        Ok(())
    }

    /// 可跑（挂闸才能跑）。
    pub const fn runnable(&self, a: B28Asset) -> bool {
        self.gated[a.idx()]
    }

    /// 全挂。
    pub fn all_attached(&self) -> bool {
        for a in B28Asset::ALL {
            if !self.runnable(a) {
                return false;
            }
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 tick 证据与门禁
// ═══════════════════════════════════════════════════════════════════════

/// 单 tick 证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickEvidence {
    /// tick 号（会话内）。
    pub tick: u64,
    /// 等价率 ppm（None 语义用 has=false 表达）。
    pub ppm: u64,
    /// 证据在（false=无证）。
    pub has: bool,
}

impl TickEvidence {
    /// 过闸（有证且满线）。
    pub const fn pass(&self) -> bool {
        self.has && self.ppm == GATE_PPM
    }
}

/// 证据流生成（breach_at: Some(t) 表示第 t tick 起破线；missing_at:
/// Some(t) 表示第 t tick 无证）。
pub fn evidence_stream(seed: u64, n: u64, breach_at: Option<u64>, missing_at: Option<u64>) -> Vec<TickEvidence> {
    let mut g = Lcg::new(seed);
    let mut out = Vec::new();
    for t in 0..n {
        let _ = g.next_u64();
        let has = match missing_at {
            Some(m) => t != m,
            None => true,
        };
        let ppm = if let Some(b) = breach_at {
            if t >= b {
                // 破线：掉到 999_000 以下（确定性递减）。
                999_000u64.saturating_sub(t.saturating_mul(10))
            } else {
                GATE_PPM
            }
        } else {
            GATE_PPM
        };
        out.push(TickEvidence { tick: t, ppm, has });
    }
    out
}

/// 门禁判定（单 tick）。
pub const fn gate_tick(ev: &TickEvidence) -> Result<(), u16> {
    if !ev.has {
        return Err(E_NO_EVIDENCE);
    }
    if ev.ppm < GATE_PPM {
        return Err(E_BREACH_PASS);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// §3 压测会话（逐 tick 过闸 + 即时拦截 + 守恒）
// ═══════════════════════════════════════════════════════════════════════

/// 会话结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionOut {
    /// 会话号。
    pub session: u64,
    /// 资产。
    pub asset: B28Asset,
    /// 计划 tick。
    pub planned: u64,
    /// 已跑 tick（过闸）。
    pub ran: u64,
    /// 被截 tick（破约点起未跑）。
    pub cut: u64,
    /// 是否全程过闸。
    pub full_pass: bool,
    /// 拦截点（破约 tick；无则 None）。
    pub intercepted_at: Option<u64>,
}

impl SessionOut {
    /// tick 守恒（计划=已跑+被截）。
    pub const fn conserved(&self) -> bool {
        self.planned == self.ran + self.cut
    }
}

/// 会话执行（证据流过门禁；即时拦截：首个未过闸 tick 当场截断）。
pub fn run_session(
    session: u64,
    asset: B28Asset,
    gate: &AssetGate,
    seed: u64,
    breach_at: Option<u64>,
    missing_at: Option<u64>,
) -> Result<SessionOut, u16> {
    if !gate.runnable(asset) {
        return Err(E_ASSET_UNGATED);
    }
    let planned = asset.plan_ticks();
    let evs = evidence_stream(seed ^ ((asset.idx() as u64) << 16), planned, breach_at, missing_at);
    let mut ran = 0u64;
    let mut intercepted_at: Option<u64> = None;
    for ev in &evs {
        match gate_tick(ev) {
            Ok(()) => ran += 1,
            Err(_) => {
                // 即时拦截：当场截断（后续 tick 不再执行）。
                intercepted_at = Some(ev.tick);
                break;
            }
        }
    }
    let cut = planned - ran;
    let full_pass = ran == planned && intercepted_at.is_none();
    Ok(SessionOut {
        session,
        asset,
        planned,
        ran,
        cut,
        full_pass,
        intercepted_at,
    })
}

/// 会话账（号单调 + 守恒 + 拦截入账）。
pub struct SessionBook {
    pub outs: Vec<SessionOut>,
    next_session: u64,
    /// 拦截入账数。
    pub intercepts: u64,
}

impl SessionBook {
    /// 建。
    pub fn new() -> Self {
        SessionBook { outs: Vec::new(), next_session: 1, intercepts: 0 }
    }

    /// 开会话（号单调分配；拦截必入账）。
    pub fn open(
        &mut self,
        gate: &AssetGate,
        asset: B28Asset,
        seed: u64,
        breach_at: Option<u64>,
        missing_at: Option<u64>,
    ) -> Result<&SessionOut, u16> {
        let sid = self.next_session;
        let out = run_session(sid, asset, gate, seed, breach_at, missing_at)?;
        self.next_session += 1;
        if out.intercepted_at.is_some() {
            self.intercepts += 1;
        }
        self.outs.push(out);
        match self.outs.last() {
            Some(o) => Ok(o),
            None => Err(E_SESSION_OOB),
        }
    }

    /// 会话号单调核。
    pub fn seq_intact(&self) -> bool {
        for (i, o) in self.outs.iter().enumerate() {
            if o.session != (i as u64 + 1) {
                return false;
            }
        }
        self.next_session == self.outs.len() as u64 + 1
    }

    /// 全会话守恒。
    pub fn all_conserved(&self) -> bool {
        self.outs.iter().all(|o| o.conserved())
    }

    /// 拦截不即时判据核（被截会话 ran < planned 且截点 < 计划尾）。
    pub fn intercepts_immediate(&self) -> bool {
        for o in &self.outs {
            if let Some(at) = o.intercepted_at {
                if o.ran != at || at >= o.planned {
                    return false;
                }
            }
        }
        true
    }

    /// 会话数。
    pub fn len(&self) -> usize {
        self.outs.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4 演练
// ═══════════════════════════════════════════════════════════════════════

/// 门禁常驻执行率 100% 演练（三资产诚实全程过闸）。
pub fn drill_gate_resident() -> (bool, String) {
    let mut gate = AssetGate::new();
    for a in B28Asset::ALL {
        if gate.attach(a).is_err() {
            return (false, "挂闸失败".to_string());
        }
    }
    let mut book = SessionBook::new();
    for (i, a) in B28Asset::ALL.iter().enumerate() {
        match book.open(&gate, *a, 0x1335_0000 + i as u64, None, None) {
            Ok(o) => {
                if !o.full_pass || !o.conserved() {
                    return (false, format!("{} 未全程过闸", a.text()));
                }
            }
            Err(c) => return (false, format!("会话失败 {:#06X}", c)),
        }
    }
    (
        gate.all_attached()
            && book.len() == 3
            && book.seq_intact()
            && book.all_conserved()
            && book.intercepts == 0,
        format!(
            "3资产全程过闸 会话{} 守恒={}",
            book.len(),
            book.all_conserved()
        ),
    )
}

/// 即时拦截 10/10 演练（逐轮注破约必拦必截必入账）。
pub fn drill_intercept_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut notes: Vec<String> = Vec::new();
    let mut gate = AssetGate::new();
    let _ = gate.attach(B28Asset::ExtremeRate);
    let mut book = SessionBook::new();
    for r in 0..INJECT_ROUNDS {
        let at = (r as u64 % 32) + 8; // 破约点 8..39
        let o = book.open(
            &gate,
            B28Asset::ExtremeRate,
            0x1335_1000 + r as u64,
            Some(at),
            None,
        );
        match o {
            Ok(out) => {
                let hit = out.intercepted_at == Some(at)
                    && out.ran == at
                    && out.cut == out.planned - at
                    && out.conserved()
                    && !out.full_pass;
                if hit {
                    ok += 1;
                }
                notes.push(format!(
                    "r{}@{}:{}",
                    r,
                    at,
                    if hit { "拦" } else { "漏" }
                ));
            }
            Err(c) => notes.push(format!("r{}:err{:#06X}", r, c)),
        }
    }
    (
        ok == INJECT_ROUNDS as u32 && book.intercepts == INJECT_ROUNDS as u64,
        format!("{}/{} {}", ok, INJECT_ROUNDS, notes.join(" ")),
    )
}

/// 无证必拦演练（缺证 tick 当场截断）。
pub fn drill_missing_evidence() -> (bool, String) {
    let mut gate = AssetGate::new();
    let _ = gate.attach(B28Asset::MultiCore);
    let mut book = SessionBook::new();
    let o = match book.open(
        &gate,
        B28Asset::MultiCore,
        0x1335_2000,
        None,
        Some(17),
    ) {
        Ok(o) => (o.intercepted_at, o.ran, o.cut, o.conserved()),
        Err(c) => return (false, format!("会话失败 {:#06X}", c)),
    };
    let hit = o.0 == Some(17) && o.1 == 17 && o.3;
    (
        hit && book.intercepts == 1,
        format!("无证@17 拦={} 已跑={} 被截={}", hit, o.1, o.2),
    )
}

/// 裸跑必拒演练（未挂闸资产开跑必拒）。
pub fn drill_ungated_refused() -> (bool, String) {
    let gate = AssetGate::new();
    let mut book = SessionBook::new();
    let refused = matches!(
        book.open(&gate, B28Asset::LongStable, 1, None, None),
        Err(E_ASSET_UNGATED)
    );
    let empty = book.len() == 0;
    // 挂一类跑另一类仍拒。
    let mut gate2 = AssetGate::new();
    let _ = gate2.attach(B28Asset::ExtremeRate);
    let refused2 = matches!(
        book.open(&gate2, B28Asset::LongStable, 1, None, None),
        Err(E_ASSET_UNGATED)
    );
    (
        refused && empty && refused2,
        format!("全裸拒={} 半挂拒={} 账空={}", refused, refused2, empty),
    )
}

/// 拔闸必拒演练（守卫恒开：证据缺省单 tick 必拦）。
pub fn drill_gate_cannot_off() -> (bool, String) {
    // 门禁判定不依赖任何开关：无证必拦、破线必拦（守卫不可关闭）。
    let ev_ok = TickEvidence { tick: 0, ppm: GATE_PPM, has: true };
    let ev_none = TickEvidence { tick: 1, ppm: GATE_PPM, has: false };
    let ev_low = TickEvidence { tick: 2, ppm: 500_000, has: true };
    (
        matches!(gate_tick(&ev_ok), Ok(()))
            && matches!(gate_tick(&ev_none), Err(E_NO_EVIDENCE))
            && matches!(gate_tick(&ev_low), Err(E_BREACH_PASS)),
        "守卫恒开：过/无证/破线三态判死".to_string(),
    )
}

/// 集成断言常绿演练（三资产各一次全程过闸 + 拦截会话不污染集成面）。
pub fn drill_integration() -> (bool, String) {
    let mut gate = AssetGate::new();
    let _ = gate.attach(B28Asset::ExtremeRate);
    let _ = gate.attach(B28Asset::MultiCore);
    let _ = gate.attach(B28Asset::LongStable);
    let mut book = SessionBook::new();
    // 每资产一次诚实全程。
    let mut full = [false; 3];
    for (i, a) in B28Asset::ALL.iter().enumerate() {
        match book.open(&gate, *a, 0x1335_3000 + i as u64, None, None) {
            Ok(o) => full[a.idx()] = o.full_pass,
            Err(_) => {}
        }
    }
    // 再来一次破约会话（不得影响集成面判定：破约仍守恒）。
    let dirty = book.open(&gate, B28Asset::ExtremeRate, 0x1335_3000 + 9, Some(5), None);
    let dirty_ok = matches!(dirty, Ok(o) if o.conserved() && !o.full_pass);
    (
        full[0] && full[1] && full[2] && dirty_ok && book.all_conserved() && book.intercepts == 1,
        format!(
            "集成绿={:?} 破约守恒={} 拦截={}",
            full,
            dirty_ok,
            book.intercepts
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §4b 拦截明细账（逐拦截留痕 + 序单调 + 导出）
// ═══════════════════════════════════════════════════════════════════════

/// 拦截明细。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterceptRec {
    /// 会话号。
    pub session: u64,
    /// 资产序。
    pub asset: usize,
    /// 破约 tick。
    pub at: u64,
    /// 观测 ppm。
    pub ppm: u64,
}

/// 拦截明细账。
pub struct InterceptLedger {
    recs: Vec<InterceptRec>,
}

impl InterceptLedger {
    /// 建。
    pub fn new() -> Self {
        InterceptLedger { recs: Vec::new() }
    }

    /// 记一笔（ppm 必 <10^6，拦截语义自洽）。
    pub fn record(&mut self, session: u64, asset: usize, at: u64, ppm: u64) -> Result<(), u16> {
        if ppm >= GATE_PPM {
            return Err(E_BREACH_PASS);
        }
        if session == 0 {
            return Err(E_SESSION_SEQ);
        }
        self.recs.push(InterceptRec { session, asset, at, ppm });
        Ok(())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 会话序单调（拦截随会话号不减）。
    pub fn session_monotone(&self) -> bool {
        for i in 1..self.recs.len() {
            if self.recs[i].session < self.recs[i - 1].session {
                return false;
            }
        }
        true
    }

    /// 导出行集。
    pub fn export(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("拦截明细 共{}", self.recs.len()));
        for r in &self.recs {
            out.push(format!(
                "会话{} 资产{} tick{} ppm={}",
                r.session, r.asset, r.at, r.ppm
            ));
        }
        out
    }
}

/// 拦截明细演练（10 次拦截全留痕 + ppm 合法性 + 导出稳定）。
pub fn drill_intercept_ledger() -> (bool, String) {
    let mut gate = AssetGate::new();
    let _ = gate.attach(B28Asset::ExtremeRate);
    let mut book = SessionBook::new();
    let mut led = InterceptLedger::new();
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let at = (r as u64 % 16) + 4;
        match book.open(&gate, B28Asset::ExtremeRate, 0x1335_4000 + r as u64, Some(at), None) {
            Ok(o) => {
                if let Some(t) = o.intercepted_at {
                    let ppm = GATE_PPM - 1 - t; // 破线观测（<10^6）
                    if led.record(o.session, o.asset.idx(), t, ppm).is_ok() {
                        ok += 1;
                    }
                }
            }
            Err(_) => {}
        }
    }
    let export_ok = led.export().len() == INJECT_ROUNDS + 1;
    let export2 = led.export();
    (
        ok == INJECT_ROUNDS as u32
            && led.len() == INJECT_ROUNDS
            && led.session_monotone()
            && export_ok
            && led.export() == export2,
        format!(
            "{}/{} 留痕 序单调={} 导出稳={}",
            ok,
            INJECT_ROUNDS,
            led.session_monotone(),
            export_ok
        ),
    )
}

/// 拦截账+拒收合并演练。
pub fn drill_intercept_book() -> (bool, String) {
    let (a, da) = drill_intercept_ledger();
    let (b, db) = drill_intercept_refuse();
    (a && b, format!("留痕={} 拒收={} | {} {}", a, b, da, db))
}

/// tick 普查+过闸率合并演练。
pub fn drill_census_rate() -> (bool, String) {
    let (a, da) = drill_tick_census();
    let (b, db) = drill_gate_rate();
    (a && b, format!("三分账={} 过闸率={} | {} {}", a, b, da, db))
}

/// 拒收三合并演练（无证/裸跑/恒开）。
pub fn drill_refuse_3() -> (bool, String) {
    let (a, da) = drill_missing_evidence();
    let (b, db) = drill_ungated_refused();
    let (c, dc) = drill_gate_cannot_off();
    (a && b && c, format!("无证拦={} 裸跑拒={} 恒开={} | {} {} {}", a, b, c, da, db, dc))
}

/// 拦截账拒收演练（满线 ppm 禁入拦截账）。
pub fn drill_intercept_refuse() -> (bool, String) {
    let mut led = InterceptLedger::new();
    let refused = matches!(led.record(1, 0, 0, GATE_PPM), Err(E_BREACH_PASS));
    let zero_sess = matches!(led.record(0, 0, 0, 999), Err(E_SESSION_SEQ));
    let ok1 = led.record(2, 1, 3, 123_456).is_ok();
    (
        refused && zero_sess && ok1 && led.len() == 1,
        format!("满线拒={} 零会话拒={} 合格入={}", refused, zero_sess, ok1),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §4c 门禁 tick 普查（过/破/无证三分账 + 守恒）
// ═══════════════════════════════════════════════════════════════════════

/// tick 普查。
#[derive(Debug, PartialEq, Eq)]
pub struct TickCensus {
    /// 过闸数。
    pub pass: u64,
    /// 破线数。
    pub breach: u64,
    /// 无证数。
    pub noevid: u64,
}

/// 对证据流普查（逐 tick 判定分流）。
pub fn tick_census(evs: &[TickEvidence]) -> TickCensus {
    let mut c = TickCensus { pass: 0, breach: 0, noevid: 0 };
    for ev in evs {
        match gate_tick(ev) {
            Ok(()) => c.pass += 1,
            Err(E_NO_EVIDENCE) => c.noevid += 1,
            Err(_) => c.breach += 1,
        }
    }
    c
}

/// tick 普查演练（三分账守恒 + 破约流/缺证流/干净流三态）。
pub fn drill_tick_census() -> (bool, String) {
    // 干净流 64 tick。
    let clean = evidence_stream(0x1335_5000, 64, None, None);
    let c1 = tick_census(&clean);
    let clean_ok = c1.pass == 64 && c1.breach == 0 && c1.noevid == 0;
    // 破约流：自 tick 10 起破线。
    let dirty = evidence_stream(0x1335_5001, 64, Some(10), None);
    let c2 = tick_census(&dirty);
    let dirty_ok = c2.pass == 10 && c2.breach == 54 && c2.noevid == 0;
    // 缺证流：tick 5 无证。
    let miss = evidence_stream(0x1335_5002, 32, None, Some(5));
    let c3 = tick_census(&miss);
    let miss_ok = c3.pass == 31 && c3.noevid == 1 && c3.breach == 0;
    // 守恒：三分相加 == 总 tick。
    let cons = |c: &TickCensus, n: u64| c.pass + c.breach + c.noevid == n;
    (
        clean_ok
            && dirty_ok
            && miss_ok
            && cons(&c1, 64)
            && cons(&c2, 64)
            && cons(&c3, 32),
        format!(
            "净{} 破{} 缺{} 守恒={}",
            clean_ok, dirty_ok, miss_ok,
            cons(&c1, 64) && cons(&c2, 64) && cons(&c3, 32)
        ),
    )
}

/// 过闸率演练（干净流过闸率 10^6；破约流按计划截断点计量）。
pub fn drill_gate_rate() -> (bool, String) {
    let clean = evidence_stream(0x1335_6000, 96, None, None);
    let c = tick_census(&clean);
    let rate = c.pass.saturating_mul(1_000_000) / 96;
    // 破约流在 @20 截断：过闸 20/96（会话视角 ran==20）。
    let mut gate = AssetGate::new();
    let _ = gate.attach(B28Asset::LongStable);
    let mut book = SessionBook::new();
    let ran = match book.open(&gate, B28Asset::LongStable, 0x1335_6001, Some(20), None) {
        Ok(o) => o.ran,
        Err(_) => u64::MAX,
    };
    (
        rate == 1_000_000 && ran == 20,
        format!("净流过闸ppm={} 破约会话已跑={}", rate, ran),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（无证/破线/裸跑/不即时/失序/守恒）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：无证 tick 必拦。
    let ev = TickEvidence { tick: 0, ppm: GATE_PPM, has: false };
    if matches!(gate_tick(&ev), Err(E_NO_EVIDENCE)) {
        caught += 1;
        det.push("无证:拦".to_string());
    } else {
        det.push("无证:漏".to_string());
    }
    // 注2：破线必拦。
    let ev2 = TickEvidence { tick: 0, ppm: 999_999, has: true };
    if matches!(gate_tick(&ev2), Err(E_BREACH_PASS)) {
        caught += 1;
        det.push("破线:拦".to_string());
    } else {
        det.push("破线:漏".to_string());
    }
    // 注3：裸跑必拒。
    let gate = AssetGate::new();
    let mut book = SessionBook::new();
    if matches!(book.open(&gate, B28Asset::MultiCore, 1, None, None), Err(E_ASSET_UNGATED)) {
        caught += 1;
        det.push("裸跑:拒".to_string());
    } else {
        det.push("裸跑:漏".to_string());
    }
    // 注4：拦截即时性（被截会话 ran==截点）。
    let mut gate2 = AssetGate::new();
    let _ = gate2.attach(B28Asset::ExtremeRate);
    let mut book2 = SessionBook::new();
    match book2.open(&gate2, B28Asset::ExtremeRate, 2, Some(3), None) {
        Ok(o) if o.ran == 3 && o.intercepted_at == Some(3) => {
            caught += 1;
            det.push("即时:是".to_string());
        }
        _ => det.push("即时:漏".to_string()),
    }
    // 注5：会话号单调。
    let mut gate3 = AssetGate::new();
    for a in B28Asset::ALL {
        let _ = gate3.attach(a);
    }
    let mut book3 = SessionBook::new();
    for (i, a) in B28Asset::ALL.iter().enumerate() {
        let _ = book3.open(&gate3, *a, 3 + i as u64, None, None);
    }
    if book3.seq_intact() && book3.len() == 3 {
        caught += 1;
        det.push("序:守".to_string());
    } else {
        det.push("序:漏".to_string());
    }
    // 注6：tick 守恒（破约会话 planned==ran+cut）。
    let mut book4 = SessionBook::new();
    let cons = match book4.open(&gate3, B28Asset::LongStable, 9, Some(10), None) {
        Ok(o) => o.conserved() && o.ran + o.cut == o.planned,
        Err(_) => false,
    };
    if cons {
        caught += 1;
        det.push("守恒:守".to_string());
    } else {
        det.push("守恒:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §6 run_unx1335_checks：本单判据聚合（诊断段 0x7750）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：门禁常驻执行率 100%、破约即时拦截 10/10、集成断言常绿。
pub fn run_unx1335_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1335 等价与B28衔接");
    let (ok1, d1) = drill_gate_resident();
    s.add_dyn("J1-门禁常驻全程过闸", ok1, &d1);
    let (ok2, d2) = drill_intercept_10();
    s.add_dyn("J2-破约即时拦截10/10", ok2, &d2);
    let (ok3, d3) = drill_refuse_3();
    s.add_dyn("J3-无证拦+裸跑拒+恒开", ok3, &d3);
    let (ok4, d4) = drill_integration();
    s.add_dyn("J4-集成断言常绿", ok4, &d4);
    let (ok5, d5) = drill_intercept_book();
    s.add_dyn("J5-拦截明细账", ok5, &d5);
    let (ok6, d6) = drill_census_rate();
    s.add_dyn("J6-tick三分账+过闸率", ok6, &d6);
    let (c7, t7, d7) = inject_core();
    s.add_dyn(
        "J7-注入核6/6",
        c7 == t7,
        &format!("{}/{} {}", c7, t7, d7.join(" ")),
    );
    s.add("J8-段位守卫", segment_guard(), "0x7750段闭集10码");
    s.add(
        "J9-宇宙常量",
        ASSET_COUNT == 3 && GATE_PPM == 1_000_000,
        "3资产×100%线写死",
    );
    s
}
