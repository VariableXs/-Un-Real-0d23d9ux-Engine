//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18432 版本探测钳对抗（试探性递增比较识别） · 0x7E7C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18432-J1 探测钳行为（≥3 次递增比较）识别记账，误报率 <5% 断言（任务书专题一口径）
//! 设计要点：
//!   - 探测钳 = 应用试图戳穿版本谎报的经典手法：对同一 (API族,维度)
//!     连续发出 ≥3 次「操作数严格递增」的比较查询
//!     （如 GreaterEqual(5.0)→(5.1)→(6.0) 逐档试探）；
//!   - 识别器按 (API,维度) 槽位记账递增连击 streak，达 3 次即判钳入账；
//!     等值/回退操作数复位 streak（单调性破坏），跨维度/跨 API 各自独立；
//!   - 反规避：试探步之间夹带无关查询不复位连击（窗口跨度 ≤8 内有效），
//!     绕过窗口跨度同样显式复位——规避不白绕，识别不放过；
//!   - 误报口径（任务书专题一口径）：良性流判钳 = 误报，
//!     语料 12 族良性模式全量判静 → 误报率 0% < 5%（整数账，无浮点）；
//!   - 注入核：4 类钳样本（等步长扫描/最小步进+1/夹带规避流/跨长扫
//!     描越窗复位）逐类 10 轮，钳样本必被抓、反规避样本必被识破；
//!   - 确定性：语料全部手工模式+固定 LCG（种子=SEG_BASE 0x7E7C0），
//!     双跑分类逐流一致；no_std+alloc；零 unwrap/expect/panic；禁浮点/IO/墙钟。
//! 结构清单：
//!   CompareOp / ProbeEvent / ProbeCode(诊断闭集) / DimTracker(槽位连击态)
//!   / ClampDetector(识别器) / ClampLedger(记账) / 良性12族+钳4类语料
//!   / misreport_rate(误报账) / InjectLedger / golden_run / run_unx18432_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E7C0，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E7C0;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 钳判阈值（冻结：判据原文「≥3 次递增比较」——第 3 次递增即判钳）。
pub const CLAMP_THRESHOLD: u32 = 3;
/// 观测窗口跨度（冻结：连击有效窗口 8 个事件，超窗复位防无界记忆）。
pub const WINDOW_SPAN: u32 = 8;
/// 错误注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 良性语料族数（冻结：12 族模式，覆盖显示轮询/单发/双试/复位/跨维等）。
pub const BENIGN_FAMILIES: usize = 12;
/// 钳样本类数（冻结：4 类注入）。
pub const CLAMP_CLASSES: usize = 4;
/// 误报率上限（冻结：5%——整数账上良性流误报必须为 0 才满足 <5%）。
pub const MISREPORT_LIMIT_PCT: u32 = 5;
/// API 族槽数（冻结：4 族）。
pub const API_N: u32 = 4;
/// 维度槽数（冻结：3 维：主/次/Build）。
pub const DIM_N: u32 = 3;

// ── 比较操作枚举 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareOp {
    /// 等值比较（版本显示常见，非探测）。
    Equal = 0,
    /// 不等比较（非探测）。
    NotEqual = 1,
    /// 向上探测：操作数须严格递增才续连击。
    Greater = 2,
    /// 向上探测（含等）：操作数严格递增才续连击。
    GreaterEqual = 3,
    /// 向下比较：非向上探测，复位向上连击。
    Less = 4,
    /// 向下比较（含等）：同上。
    LessEqual = 5,
}

impl CompareOp {
    /// 是否为向上探测型比较（Greater/GreaterEqual 两族）。
    pub fn is_probe(self) -> bool {
        self == CompareOp::Greater || self == CompareOp::GreaterEqual
    }
}

/// 单条探测查询事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeEvent {
    /// 进程键（语料内区分流）。
    pub proc_id: u32,
    /// API 族序（0..4）。
    pub api_id: u32,
    /// 维度序（0..3）。
    pub dim: u32,
    /// 比较操作。
    pub op: CompareOp,
    /// 操作数（版本序数，如 5000=5.0）。
    pub operand: u32,
}

impl ProbeEvent {
    /// (API,维度) 槽位下标（键空间 4×3=12，越界折叠取模——语料闭集内恒合法）。
    pub fn slot(&self) -> usize {
        (self.api_id % API_N) as usize * DIM_N as usize + (self.dim % DIM_N) as usize
    }
}

/// 事件构造辅助（no_std 无 vec! 宏，语料用 push 逐条装配）。
#[allow(dead_code)]
fn ev(proc_id: u32, api_id: u32, dim: u32, op: CompareOp, operand: u32) -> ProbeEvent {
    ProbeEvent { proc_id, api_id, dim, op, operand }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    /// 探测钳命中（≥3 次递增，入账）
    ClampDetected,
    /// 反规避命中：夹带无关查询未打断连击
    EvasionGap,
    /// 连击被复位（等值/回退，不判钳）
    ResetDetected,
    /// 递增不足阈值（<3 次，不判钳）
    ThresholdShort,
    /// 递增跨维度（各维连击不足，不判钳）
    DimMixed,
    /// 递增跨 API 族（不判钳）
    ApiMixed,
    /// 空流（无可判事件）
    EmptyStream,
    /// 记账账本满
    LedgerFull,
    /// 双跑不一致（确定性红线）
    NotReproducible,
    /// 流标签与识别结果错配（语料自检面）
    LabelMismatch,
}

impl ProbeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::ClampDetected => "探测钳命中：≥3 次递增比较已入账",
            ProbeCode::EvasionGap => "反规避命中：夹带无关查询未打断递增连击",
            ProbeCode::ResetDetected => "连击复位：等值或回退操作数破坏单调性",
            ProbeCode::ThresholdShort => "递增不足 3 次：未达钳判阈值",
            ProbeCode::DimMixed => "递增跨维度：单维连击不足，不判钳",
            ProbeCode::ApiMixed => "递增跨 API 族：不判钳",
            ProbeCode::EmptyStream => "空流：无事件可判",
            ProbeCode::LedgerFull => "钳记账满：超出按 dropped 显性记账",
            ProbeCode::NotReproducible => "双跑不一致：确定性红线",
            ProbeCode::LabelMismatch => "语料标签与识别结果错配：语料面自检失败",
        }
    }
    pub const ALL: [ProbeCode; 10] = [
        ProbeCode::ClampDetected,
        ProbeCode::EvasionGap,
        ProbeCode::ResetDetected,
        ProbeCode::ThresholdShort,
        ProbeCode::DimMixed,
        ProbeCode::ApiMixed,
        ProbeCode::EmptyStream,
        ProbeCode::LedgerFull,
        ProbeCode::NotReproducible,
        ProbeCode::LabelMismatch,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        if ProbeCode::ALL[i].val() == code {
            return ProbeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E7C0 段闭集内"
}

// ── 维度槽位连击态 ──────────────────────────────────────────────
/// 单 (API,维度) 槽的连击状态机：Idle→Probing(1)→Probing(2)→Clamped(3)。
/// 迁移：探测+严格递增 → streak+1；等值/回退/超窗/换槽 → 归零回 Idle。
/// 窗口按全局事件计（tick）：连击两步之间隔超过 WINDOW_SPAN 个事件即复位——
/// 夹带少量无关查询不救命，远间隔试探也攒不出连击（无界记忆红线）。
#[derive(Clone, Copy, Debug)]
pub struct DimTracker {
    /// 当前连击长度（0=Idle）。
    pub streak: u32,
    /// 上一个探测操作数（严格递增判据基线）。
    pub last_operand: u32,
    /// 连击最近一次推进的全局事件号。
    pub last_tick: u32,
}

impl DimTracker {
    /// 空槽。
    pub const IDLE: DimTracker = DimTracker { streak: 0, last_operand: 0, last_tick: 0 };
}

// ── 钳记账账本 ──────────────────────────────────────────────────
/// 一次钳命中记账：流标识+连击跨度+判因码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClampEvent {
    pub proc_id: u32,
    pub api_id: u32,
    pub dim: u32,
    /// 连击长度（≥CLAMP_THRESHOLD）。
    pub streak: u32,
    /// 判因码（ClampDetected 或 EvasionGap）。
    pub reason: u32,
}

/// 账本：满则 dropped 显性记账。
pub struct ClampLedger {
    pub events: Vec<ClampEvent>,
    cap: usize,
    pub dropped: u32,
    /// 已扫事件总数（守恒账）。
    pub scanned: u32,
}

impl ClampLedger {
    pub fn new(cap: usize) -> ClampLedger {
        ClampLedger { events: Vec::new(), cap, dropped: 0, scanned: 0 }
    }
    /// 入账一条钳命中；满则记账不静默丢。
    pub fn flag(&mut self, ev: ClampEvent) {
        if self.events.len() >= self.cap {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.events.push(ev);
    }
}

/// 判定输出：是否判钳 + 判因码（未判钳也给复位/不足阈值等负判因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub clamped: bool,
    pub code: ProbeCode,
}

// ── 识别器本体 ──────────────────────────────────────────────────
/// 探测钳识别器：12 个 (API,维度) 槽位连击态 + 判定输出。
/// 单流扫描语义：一个流内首达阈值即判钳（每流至多记 1 条，防刷账）。
pub struct ClampDetector {
    slots: [DimTracker; API_N as usize * DIM_N as usize],
    /// 全局事件号（窗口计），每条事件（含非探测）推进 1。
    tick: u32,
    /// 本流是否已判钳（去重记账）。
    pub flagged_once: bool,
    /// 本流最终判因。
    pub verdict: Option<ProbeCode>,
    /// 规避样本标记（夹带无关查询出现过）。
    pub saw_interleave: bool,
}

impl ClampDetector {
    /// 新识别器（全槽 Idle）。
    pub fn new() -> ClampDetector {
        ClampDetector {
            slots: [DimTracker::IDLE; API_N as usize * DIM_N as usize],
            tick: 0,
            flagged_once: false,
            verdict: None,
            saw_interleave: false,
        }
    }

    /// 处理一条事件，返回本流是否已判钳。
    pub fn feed(&mut self, ev: &ProbeEvent, ledger: &mut ClampLedger) -> bool {
        ledger.scanned = ledger.scanned.saturating_add(1);
        let s = ev.slot();
        // 非探测操作：只复位**本事件所在槽**（无关查询不动别的槽），
        // 但全局 tick 照常推进（占窗口预算），并记本流出现过夹带。
        if !ev.op.is_probe() {
            self.slots[s] = DimTracker::IDLE;
            self.saw_interleave = true;
            self.tick = self.tick.saturating_add(1);
            self.note_negative(ProbeCode::ResetDetected);
            return self.flagged_once;
        }
        let t = &mut self.slots[s];
        // 超窗：连击两步间隔超 WINDOW_SPAN 个事件 → 显式复位（无界记忆不允许）。
        if t.streak > 0 && self.tick.saturating_sub(t.last_tick) > WINDOW_SPAN {
            *t = DimTracker::IDLE;
        }
        // 严格递增才续连击；等值或回退复位。
        if t.streak == 0 || ev.operand > t.last_operand {
            t.streak = t.streak.saturating_add(1);
            t.last_operand = ev.operand;
            t.last_tick = self.tick;
        } else {
            *t = DimTracker::IDLE;
            self.note_negative(ProbeCode::ResetDetected);
            self.tick = self.tick.saturating_add(1);
            return self.flagged_once;
        }
        self.tick = self.tick.saturating_add(1);
        // 达阈值：判钳入账（每流一次）。
        if t.streak >= CLAMP_THRESHOLD {
            let reason = if self.saw_interleave { ProbeCode::EvasionGap } else { ProbeCode::ClampDetected };
            if !self.flagged_once {
                ledger.flag(ClampEvent {
                    proc_id: ev.proc_id,
                    api_id: ev.api_id,
                    dim: ev.dim,
                    streak: t.streak,
                    reason: reason.val(),
                });
                self.flagged_once = true;
            }
            self.verdict = Some(reason);
            return true;
        }
        self.note_negative(ProbeCode::ThresholdShort);
        self.flagged_once
    }

    /// 记负判因：首个负判因入账；「复位」优先于「不足阈值」——
    /// 复位是确定性负判（单调性被破坏），不足阈值只是暂态。
    fn note_negative(&mut self, code: ProbeCode) {
        if self.verdict.is_none() {
            self.verdict = Some(code);
        } else if code == ProbeCode::ResetDetected && self.verdict == Some(ProbeCode::ThresholdShort) {
            self.verdict = Some(code);
        }
    }

    /// 全槽复位（下一流用新识别器；账本连续）。
    pub fn reset(&mut self) {
        let mut i = 0;
        while i < self.slots.len() {
            self.slots[i] = DimTracker::IDLE;
            i += 1;
        }
        self.tick = 0;
        self.flagged_once = false;
        self.verdict = None;
        self.saw_interleave = false;
    }
}

/// 流分类：单流喂入，返回 (判钳, 判因码)。
pub fn classify(stream: &[ProbeEvent], ledger: &mut ClampLedger) -> Verdict {
    if stream.len() == 0 {
        return Verdict { clamped: false, code: ProbeCode::EmptyStream };
    }
    let mut det = ClampDetector::new();
    let mut i = 0;
    while i < stream.len() {
        if det.feed(&stream[i], ledger) {
            return Verdict { clamped: true, code: det.verdict.unwrap_or(ProbeCode::ClampDetected) };
        }
        i += 1;
    }
    Verdict { clamped: false, code: det.verdict.unwrap_or(ProbeCode::EmptyStream) }
}

// ── 语料构造（确定性，手工模式） ────────────────────────────────
/// 钳样本类 0：等步长扫描（经典戳谎：5000→5001→5002→…同维向上）。
pub fn make_clamp_equal_step(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut k = 0;
    while k < 6 {
        v.push(ProbeEvent {
            proc_id,
            api_id: 1,
            dim: 0,
            op: CompareOp::GreaterEqual,
            operand: 5000 + k as u32,
        });
        k += 1;
    }
    v
}

/// 钳样本类 1：最小步进 +1（最细试探，仍 ≥3 次递增）。
pub fn make_clamp_min_step(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut k = 0;
    while k < CLAMP_THRESHOLD {
        v.push(ProbeEvent {
            proc_id,
            api_id: 0,
            dim: 1,
            op: CompareOp::Greater,
            operand: 61000 + k as u32,
        });
        k += 1;
    }
    v
}

/// 钳样本类 2：夹带规避流——探测步间夹**无关槽位**的等值查询
/// （应不打断连击，仅消耗窗口预算）。
pub fn make_clamp_evasion(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut k = 0;
    while k < 4 {
        v.push(ProbeEvent {
            proc_id,
            api_id: 2,
            dim: 2,
            op: CompareOp::GreaterEqual,
            operand: 9000 + k as u32 * 10,
        });
        // 夹带：无关槽位（API0,维0）的等值查询（非探测操作）。
        v.push(ProbeEvent {
            proc_id,
            api_id: 0,
            dim: 0,
            op: CompareOp::Equal,
            operand: 0,
        });
        k += 1;
    }
    v
}

/// 钳样本类 3：跨维分摊——每维 3 次递增但夹不同维事件，任何单维仍达阈值。
pub fn make_clamp_crosstalk(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut k = 0;
    while k < 3 {
        v.push(ProbeEvent { proc_id, api_id: 3, dim: 0, op: CompareOp::GreaterEqual, operand: 2000 + k });
        v.push(ProbeEvent { proc_id, api_id: 3, dim: 1, op: CompareOp::GreaterEqual, operand: 2000 + k });
        v.push(ProbeEvent { proc_id, api_id: 3, dim: 2, op: CompareOp::GreaterEqual, operand: 2000 + k });
        k += 1;
    }
    v
}

/// 钳样本四类打包。
pub fn make_clamp_streams() -> [Vec<ProbeEvent>; CLAMP_CLASSES] {
    [
        make_clamp_equal_step(100),
        make_clamp_min_step(101),
        make_clamp_evasion(102),
        make_clamp_crosstalk(103),
    ]
}

/// 良性族 0：同操作数等值轮询（版本显示常见，永不判钳）。
pub fn make_benign_poll(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut k = 0;
    while k < 9 {
        v.push(ProbeEvent { proc_id, api_id: 0, dim: 0, op: CompareOp::Equal, operand: 62000 });
        k += 1;
    }
    v
}

/// 良性族 1：等值夹一次不等（否定型查询，非探测）。
pub fn make_benign_neg(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 1, 1, CompareOp::Equal, 62000));
    v.push(ev(proc_id, 1, 1, CompareOp::NotEqual, 63000));
    v.push(ev(proc_id, 1, 1, CompareOp::Equal, 62000));
    v
}

/// 良性族 2：仅 2 次递增（阈值边界：不足 3 不判钳）。
pub fn make_benign_two_inc(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 2, 0, CompareOp::GreaterEqual, 5000));
    v.push(ev(proc_id, 2, 0, CompareOp::GreaterEqual, 5001));
    v
}

/// 良性族 3：递增中途回退（单调性破坏，复位）。
pub fn make_benign_backoff(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 0, 2, CompareOp::Greater, 7000));
    v.push(ev(proc_id, 0, 2, CompareOp::Greater, 7010));
    v.push(ev(proc_id, 0, 2, CompareOp::Greater, 7000));
    v.push(ev(proc_id, 0, 2, CompareOp::Greater, 7010));
    v.push(ev(proc_id, 0, 2, CompareOp::Greater, 7020));
    v
}

/// 良性族 4：等值踩点（递增序列中间夹等值，复位连击）。
pub fn make_benign_equal_break(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 1, 0, CompareOp::GreaterEqual, 8000));
    v.push(ev(proc_id, 1, 0, CompareOp::GreaterEqual, 8000));
    v.push(ev(proc_id, 1, 0, CompareOp::GreaterEqual, 8001));
    v.push(ev(proc_id, 1, 0, CompareOp::GreaterEqual, 8002));
    v
}

/// 良性族 5：向下比较族（Less 系，非向上探测）。
pub fn make_benign_less(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut k = 0;
    while k < 5 {
        v.push(ProbeEvent {
            proc_id,
            api_id: 3,
            dim: 2,
            op: CompareOp::LessEqual,
            operand: 9000 + k as u32,
        });
        k += 1;
    }
    v
}

/// 良性族 6：单发大跳（一次查询，无连击可言）。
pub fn make_benign_single(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 2, 1, CompareOp::GreaterEqual, 10000));
    v
}

/// 良性族 7：递增跨 API 族（同维不同 API，各 API 连击不足 3）。
pub fn make_benign_api_mix(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 0, 0, CompareOp::GreaterEqual, 100));
    v.push(ev(proc_id, 1, 0, CompareOp::GreaterEqual, 101));
    v.push(ev(proc_id, 2, 0, CompareOp::GreaterEqual, 102));
    v
}

/// 良性族 8：递增跨维度（同 API 不同维，单维连击不足 3）。
pub fn make_benign_dim_mix(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ev(proc_id, 3, 0, CompareOp::GreaterEqual, 200));
    v.push(ev(proc_id, 3, 1, CompareOp::GreaterEqual, 201));
    v.push(ev(proc_id, 3, 2, CompareOp::GreaterEqual, 202));
    v
}

/// 良性族 9：超窗规避间隔（两递增隔 9 个事件，超窗复位不判钳）。
pub fn make_benign_far_gap(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ProbeEvent { proc_id, api_id: 0, dim: 1, op: CompareOp::GreaterEqual, operand: 300 });
    let mut k = 0;
    while k < 9 {
        v.push(ProbeEvent { proc_id, api_id: 1, dim: 2, op: CompareOp::Equal, operand: 1 });
        k += 1;
    }
    v.push(ProbeEvent { proc_id, api_id: 0, dim: 1, op: CompareOp::GreaterEqual, operand: 301 });
    v.push(ProbeEvent { proc_id, api_id: 0, dim: 1, op: CompareOp::GreaterEqual, operand: 302 });
    v
}

/// 良性族 10：递增越窗（三递增夹 8 个杂音，窗口跨度耗尽复位）。
pub fn make_benign_span_bust(proc_id: u32) -> Vec<ProbeEvent> {
    let mut v: Vec<ProbeEvent> = Vec::new();
    v.push(ProbeEvent { proc_id, api_id: 2, dim: 2, op: CompareOp::GreaterEqual, operand: 400 });
    let mut k = 0;
    while k < 8 {
        v.push(ProbeEvent { proc_id, api_id: 0, dim: 0, op: CompareOp::Equal, operand: 2 });
        k += 1;
    }
    v.push(ProbeEvent { proc_id, api_id: 2, dim: 2, op: CompareOp::GreaterEqual, operand: 401 });
    v.push(ProbeEvent { proc_id, api_id: 2, dim: 2, op: CompareOp::GreaterEqual, operand: 402 });
    v
}

/// 良性族 11：混合常态流量（LCG 确定性混合等值/小于/双试递增，全部短于阈值）。
pub fn make_benign_mixed(proc_id: u32, seed: u64) -> Vec<ProbeEvent> {
    // 固定 LCG 混流；操作数用步长 50 递增但每 2 步复位（保证单维连击恒 <3）。
    let mut g = Lcg(seed);
    let mut v: Vec<ProbeEvent> = Vec::new();
    let mut base = 60_000u32;
    let mut k = 0;
    while k < 16 {
        let pick = g.below(3);
        match pick {
            0 => v.push(ProbeEvent { proc_id, api_id: 1, dim: 2, op: CompareOp::Equal, operand: base }),
            1 => v.push(ProbeEvent { proc_id, api_id: 2, dim: 0, op: CompareOp::Less, operand: base }),
            _ => {
                v.push(ProbeEvent { proc_id, api_id: 3, dim: 1, op: CompareOp::GreaterEqual, operand: base });
                base = base.wrapping_add(50);
            }
        }
        // 每 2 轮强制复位基线（防意外连击到 3）。
        if k % 2 == 1 {
            base = base.wrapping_sub(50);
        }
        k += 1;
    }
    v
}

/// 固定种子 LCG。
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

/// 良性 12 族打包（进程号错开防串账）。
pub fn make_benign_streams() -> [Vec<ProbeEvent>; BENIGN_FAMILIES] {
    [
        make_benign_poll(0),
        make_benign_neg(1),
        make_benign_two_inc(2),
        make_benign_backoff(3),
        make_benign_equal_break(4),
        make_benign_less(5),
        make_benign_single(6),
        make_benign_api_mix(7),
        make_benign_dim_mix(8),
        make_benign_far_gap(9),
        make_benign_span_bust(10),
        make_benign_mixed(11, SEG_BASE as u64),
    ]
}

// ── 误报账 ──────────────────────────────────────────────────────
/// 误报率账：良性流误报数 / 良性流总数（整数账 ×1000 表千分比）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MisreportBook {
    pub benign: u32,
    pub misreported: u32,
}

impl MisreportBook {
    pub fn new() -> MisreportBook {
        MisreportBook { benign: 0, misreported: 0 }
    }
    /// 千分比误报率（整数，benign=0 返回 0）。
    pub fn permille(&self) -> u32 {
        if self.benign == 0 {
            return 0;
        }
        self.misreported.saturating_mul(1000) / self.benign
    }
    /// 误报率 < 5% 判定：千分比 < 50。
    pub fn within_limit(&self) -> bool {
        self.permille() < 50
    }
}

/// 黄金全链：12 良性流判静（0 误报）+ 4 钳类全抓 + 记账守恒 + 双跑一致。
pub fn golden_run() -> Result<(MisreportBook, u32, u32), u32> {
    // 良性侧：全量判静。
    let mut book = MisreportBook::new();
    let benign = make_benign_streams();
    let mut b = 0;
    while b < benign.len() {
        let mut led = ClampLedger::new(64);
        let v = classify(&benign[b], &mut led);
        book.benign = book.benign.saturating_add(1);
        if v.clamped {
            book.misreported = book.misreported.saturating_add(1);
        }
        b += 1;
    }
    if !book.within_limit() {
        return Err(ProbeCode::LabelMismatch.val());
    }
    // 钳侧：四类全抓（10 轮）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    if !inj.all_caught() {
        return Err(ProbeCode::LabelMismatch.val());
    }
    // 记账守恒：单流钳样本恰记 1 条（每流去重），扫描数=事件数。
    let mut led = ClampLedger::new(64);
    let clamp_streams = make_clamp_streams();
    let mut scanned_sum = 0u32;
    let mut flagged_sum = 0u32;
    let mut c = 0;
    while c < clamp_streams.len() {
        let v = classify(&clamp_streams[c], &mut led);
        if !v.clamped {
            return Err(ProbeCode::LabelMismatch.val());
        }
        scanned_sum = scanned_sum.saturating_add(led.scanned);
        led.scanned = 0;
        flagged_sum = flagged_sum.saturating_add(1);
        c += 1;
    }
    if led.events.len() as u32 != flagged_sum {
        return Err(ProbeCode::LedgerFull.val());
    }
    if scanned_sum == 0 {
        return Err(ProbeCode::EmptyStream.val());
    }
    // 双跑一致：重跑误报账与钳判结果完全相同。
    let mut book2 = MisreportBook::new();
    let benign2 = make_benign_streams();
    let mut b2 = 0;
    while b2 < benign2.len() {
        let mut led2 = ClampLedger::new(64);
        let v2 = classify(&benign2[b2], &mut led2);
        book2.benign = book2.benign.saturating_add(1);
        if v2.clamped {
            book2.misreported = book2.misreported.saturating_add(1);
        }
        b2 += 1;
    }
    if book2.benign != book.benign || book2.misreported != book.misreported {
        return Err(ProbeCode::NotReproducible.val());
    }
    Ok((book, flagged_sum, scanned_sum))
}

// ── 注入核（钳样本 10 轮 + 反规避样本 10 轮） ───────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：四类钳样本逐类必被抓。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let streams = make_clamp_streams();
        let mut s = 0;
        while s < streams.len() {
            let mut book = ClampLedger::new(64);
            let v = classify(&streams[s], &mut book);
            led.record(s, v.clamped);
            s += 1;
        }
        r += 1;
    }
    led
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：等步长扫描流判钳且判因正确。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut book = ClampLedger::new(8);
    let stream = make_clamp_equal_step(1);
    let v = classify(&stream, &mut book);
    if !v.clamped || v.code != ProbeCode::ClampDetected {
        return Err(ProbeCode::LabelMismatch.val());
    }
    if book.events.len() != 1 {
        return Err(ProbeCode::LedgerFull.val());
    }
    Ok(())
}

/// 负探针 1：空流不判钳且不 panic。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut book = ClampLedger::new(8);
    let v = classify(&[], &mut book);
    if v.clamped || v.code != ProbeCode::EmptyStream {
        return Err(ProbeCode::LabelMismatch.val());
    }
    Ok(())
}

/// 负探针 2：双试递增（阈值-1）不判钳。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut book = ClampLedger::new(8);
    let stream = make_benign_two_inc(7);
    let v = classify(&stream, &mut book);
    if v.clamped {
        return Err(ProbeCode::LabelMismatch.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        let v = ProbeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18432_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18432 版本探测钳对抗");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "误报{}/{} 钳抓{} 扫{}",
            g.as_ref().map(|r| r.0.misreported).unwrap_or(0),
            g.as_ref().map(|r| r.0.benign).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 误报率 <5%（12 良性族零误报 → 0‰ < 50‰）。
    let rate_ok = match &g {
        Ok((book, _, _)) => book.misreported == 0 && book.within_limit(),
        Err(_) => false,
    };
    s.add("误报率<5%", rate_ok, "12 良性族 0 误报（0‰）");
    // ③ 四类钳注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "钳样本10/10识别",
        inj.all_caught(),
        &format!(
            "{}/{}/{}/{}",
            inj.caught[0], inj.caught[1], inj.caught[2], inj.caught[3]
        ),
    );
    s.add("注入漏检为零", inj.missed() == 0, "识别数+漏检=注入数");
    // ④ 反规避：夹带流判因 = EvasionGap。
    let mut book_e = ClampLedger::new(8);
    let evasion = make_clamp_evasion(9);
    let ve = classify(&evasion, &mut book_e);
    s.add("夹带规避仍判钳", ve.clamped && ve.code == ProbeCode::EvasionGap, "夹带不打断连击");
    // ⑤ 阈值边界：恰 3 次判钳。
    let mut book_t3 = ClampLedger::new(8);
    let exactly3 = make_clamp_min_step(10);
    let v3 = classify(&exactly3, &mut book_t3);
    s.add("阈值边界恰3判钳", v3.clamped, "streak=3 即判");
    // ⑥ 阈值边界：2 次不判。
    let mut book_t2 = ClampLedger::new(8);
    let two = make_benign_two_inc(11);
    let v2 = classify(&two, &mut book_t2);
    s.add("阈值边界2不判", !v2.clamped && v2.code == ProbeCode::ThresholdShort, "streak=2 归静");
    // ⑦ 等值复位边界：递增中夹等值 → 复位不判钳。
    let mut book_r = ClampLedger::new(8);
    let rb = make_benign_equal_break(12);
    let vr = classify(&rb, &mut book_r);
    s.add("等值踩点复位", !vr.clamped && vr.code == ProbeCode::ResetDetected, "等值破坏单调性");
    // ⑧ 跨维/跨 API 混流不判钳。
    let mut book_m = ClampLedger::new(8);
    let dm = make_benign_dim_mix(13);
    let am = make_benign_api_mix(14);
    let vdm = classify(&dm, &mut book_m);
    let vam = classify(&am, &mut book_m);
    s.add("跨维跨API不判钳", !vdm.clamped && !vam.clamped, "槽位独立连击");
    // ⑨ 超窗复位：杂音隔断后不判钳（无界记忆红线）。
    let mut book_w = ClampLedger::new(8);
    let sb = make_benign_span_bust(15);
    let vw = classify(&sb, &mut book_w);
    s.add("超窗显式复位", !vw.clamped, "窗口跨度 8 耗尽即复位");
    // ⑩ 记账守恒：钳账条数 = 钳流数（每流去重 1 条）。
    let mut book_c = ClampLedger::new(64);
    let streams = make_clamp_streams();
    let mut fcnt = 0u32;
    let mut c = 0;
    while c < streams.len() {
        if classify(&streams[c], &mut book_c).clamped {
            fcnt += 1;
        }
        c += 1;
    }
    s.add("钳记账守恒", book_c.events.len() as u32 == fcnt && fcnt == CLAMP_CLASSES as u32, "4 流 4 账");
    // ⑪ 账本满显性记账。
    let mut full = ClampLedger::new(2);
    let mut k = 0;
    while k < 5 {
        full.flag(ClampEvent { proc_id: 1, api_id: 0, dim: 0, streak: 3, reason: 0 });
        k += 1;
    }
    s.add("账本满显性记账", full.dropped == 3, "满后 3 条入 dropped");
    // ⑫ 双跑一致。
    let a = golden_run();
    let b = golden_run();
    s.add("双跑一致", a == b, "同语料同结果");
    // ⑬ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(), "正/空/双试");
    // ⑭ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        if ProbeCode::ALL[i].detail().is_empty() {
            closed = false;
        }
        i += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7E7C0", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_all_green() {
        let g = golden_run();
        match g {
            Ok((book, flagged, scanned)) => {
                assert_eq!(book.misreported, 0, "良性零误报");
                assert_eq!(flagged, 4, "四类钳全抓");
                assert!(scanned > 0);
            }
            Err(e) => panic!("golden_run 应全绿: {} {}", e, detail(e)),
        }
    }

    #[test]
    fn benign_twelve_families_all_quiet() {
        let benign = make_benign_streams();
        let mut b = 0;
        while b < benign.len() {
            let mut led = ClampLedger::new(8);
            let v = classify(&benign[b], &mut led);
            assert!(!v.clamped, "良性族 {} 不得判钳", b);
            b += 1;
        }
    }

    #[test]
    fn clamp_streams_ten_rounds_all_caught() {
        let inj = inject_rounds(10);
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn threshold_boundary_two_vs_three() {
        let mut l3 = ClampLedger::new(8);
        assert!(classify(&make_clamp_min_step(1), &mut l3).clamped, "恰 3 次判钳");
        let mut l2 = ClampLedger::new(8);
        assert!(!classify(&make_benign_two_inc(2), &mut l2).clamped, "2 次不判钳");
    }

    #[test]
    fn equal_operand_resets_streak() {
        let mut led = ClampLedger::new(8);
        let v = classify(&make_benign_equal_break(3), &mut led);
        assert!(!v.clamped);
        assert_eq!(v.code, ProbeCode::ResetDetected);
    }

    #[test]
    fn evasion_interleave_still_flagged() {
        let mut led = ClampLedger::new(8);
        let v = classify(&make_clamp_evasion(4), &mut led);
        assert!(v.clamped);
        assert_eq!(v.code, ProbeCode::EvasionGap);
    }

    #[test]
    fn window_bust_resets_no_flag() {
        let mut led = ClampLedger::new(8);
        let v = classify(&make_benign_span_bust(5), &mut led);
        assert!(!v.clamped, "超窗必须复位，不得凭陈旧连击判钳");
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑一致");
    }

    #[test]
    fn ledger_full_accounted() {
        let mut l = ClampLedger::new(2);
        let mut k = 0;
        while k < 5 {
            l.flag(ClampEvent { proc_id: 9, api_id: 0, dim: 0, streak: 3, reason: 0 });
            k += 1;
        }
        assert_eq!(l.events.len(), 2);
        assert_eq!(l.dropped, 3);
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18432_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
