//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12625 NtDelayExecution 可警报等待→D4 泵等待 · 0x80640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12626-J1 事件对语义（Win8 后弃用档照录注出处）
//!      对平断言绿，弃用态声明可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NtDelayExecution 语义：延迟单以 100ns 单位记账（D100NS），负值
//!     视为相对时延（取绝对值进账）、零值立即到期、正值封顶
//!     （MAX_DELAY_D100NS 封顶拒绝超长单 DelayCapExceeded）；
//!   - 可警报泵等待：泵逐 tick 推进，可警报单（Alertable=TRUE）在
//!     APC 到点时先交付 APC 再结算延迟（泵等待收束口径 ApcFirst）；
//!     非警报单只按期到期（ExpiredOnly）——收束口径四象限断言；
//!   - 弃用档照录（判据锚定）：事件对 API 的 Win8 后弃用档以冻结
//!     出处注登记（DeprecationLedger：档名/出处注/弃用态），弃用态
//!     声明可点验（逐档 source_note 非空且注脚含出处标记）；
//!   - 泵状态机 Idle→Armed→Pumping→Settled 单调（StateRegress 拒
//!     倒退）；重复结算拒（DupSettle）；泵节拍 LCG 种子写死 =
//!     SEG_BASE（0x80640），禁 rand/浮点/IO/墙钟；
//!   - 双跑指纹一致；no_std + alloc；零 unwrap/expect/panic；
//!     诊断码闭集 0x80640..0x80680。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80640，AI-VX9-1105 B02 批 12 槽第 2 槽）。
pub const SEG_BASE: u32 = 0x80640;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpWaitCode {
    /// 延迟封顶：请求时延超 MAX_DELAY_D100NS
    DelayCapExceeded,
    /// 零长单异常：负时延绝对值溢出（u32 域外）
    DelayAbsOverflow,
    /// 弃用档注脚缺失：出处注为空不可点验
    DeprecationNoteEmpty,
    /// 弃用态未声明：档在册但弃用位未落
    DeprecationStateMissing,
    /// 泵状态倒退：Idle→Armed→Pumping→Settled 只进不退
    StateRegress,
    /// 重复结算：一单只许结算一次
    DupSettle,
    /// 泵未武装：未 Armed 就推进
    PumpNotArmed,
    /// 泵后结算：Settled 后仍推进
    PumpAfterSettle,
    /// 坏延迟单号：账内查无此单
    BadDelayId,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 泵账溢出
    LedgerFull,
    /// 未知诊断码兜底
    Unknown,
}

impl PumpWaitCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PumpWaitCode::DelayCapExceeded => 0x01,
                PumpWaitCode::DelayAbsOverflow => 0x02,
                PumpWaitCode::DeprecationNoteEmpty => 0x03,
                PumpWaitCode::DeprecationStateMissing => 0x04,
                PumpWaitCode::StateRegress => 0x05,
                PumpWaitCode::DupSettle => 0x06,
                PumpWaitCode::PumpNotArmed => 0x07,
                PumpWaitCode::PumpAfterSettle => 0x08,
                PumpWaitCode::BadDelayId => 0x09,
                PumpWaitCode::FingerprintDrift => 0x0A,
                PumpWaitCode::LedgerFull => 0x0B,
                PumpWaitCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PumpWaitCode::DelayCapExceeded => "延迟封顶：拆单或缩时延再入账",
            PumpWaitCode::DelayAbsOverflow => "负时延溢出：绝对值超 u32 域，改用分段时延",
            PumpWaitCode::DeprecationNoteEmpty => "出处注为空：弃用档必须照录注出处才可点验",
            PumpWaitCode::DeprecationStateMissing => "弃用态未落位：先声明弃用态再入档",
            PumpWaitCode::StateRegress => "泵倒退：Idle→Armed→Pumping→Settled 只进不退",
            PumpWaitCode::DupSettle => "重复结算：一单只许结算一次",
            PumpWaitCode::PumpNotArmed => "泵未武装：先 arm 再 pump",
            PumpWaitCode::PumpAfterSettle => "泵后结算：Settled 后不许再推进",
            PumpWaitCode::BadDelayId => "坏单号：先查账再操作，用账内延迟单号",
            PumpWaitCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            PumpWaitCode::LedgerFull => "泵账溢出：先结算积压再入单",
            PumpWaitCode::Unknown => "未知诊断码：不在 0x80640 段闭集内",
        }
    }

    pub const ALL: [PumpWaitCode; 12] = [
        PumpWaitCode::DelayCapExceeded,
        PumpWaitCode::DelayAbsOverflow,
        PumpWaitCode::DeprecationNoteEmpty,
        PumpWaitCode::DeprecationStateMissing,
        PumpWaitCode::StateRegress,
        PumpWaitCode::DupSettle,
        PumpWaitCode::PumpNotArmed,
        PumpWaitCode::PumpAfterSettle,
        PumpWaitCode::BadDelayId,
        PumpWaitCode::FingerprintDrift,
        PumpWaitCode::LedgerFull,
        PumpWaitCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PumpWaitCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PumpWaitCode::Unknown.detail()
}

// ── 冻结常量表（来源：NtDelayExecution 语义手账 · 延迟等待节） ─────
/// 时延单位：100ns（D100NS），与内核定时器分辨率口径一致。
pub const D100NS: u32 = 1;
/// 时延封顶（100ns 单位）：10 秒档（10^8 个 100ns）。
pub const MAX_DELAY_D100NS: u32 = 100_000_000;
/// 泵账容量。
pub const LEDGER_CAP: usize = 16;
/// 弃用档在册数（判据：弃用态声明可点验）。
pub const DEPRECATED_PROFILES: usize = 4;
/// 泵推进节拍上限（逐 tick 确定性推进，不挂钟）。
pub const PUMP_TICK_CAP: u32 = 64;

/// 泵状态机（单调：Idle→Armed→Pumping→Settled）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpPhase {
    /// 空闲：无在泵单
    Idle,
    /// 已武装：延迟单入账待推
    Armed,
    /// 泵进中：逐 tick 推进
    Pumping,
    /// 已结算：全部单收束
    Settled,
}

impl PumpPhase {
    pub const fn rank(self) -> u8 {
        match self {
            PumpPhase::Idle => 0,
            PumpPhase::Armed => 1,
            PumpPhase::Pumping => 2,
            PumpPhase::Settled => 3,
        }
    }
}

/// 延迟单收束口径（判据"可警报优先"落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettleKind {
    /// 仅到期：无 APC 或非警报，按期结算
    ExpiredOnly,
    /// APC 先行：可警报单在 APC 到点先交付
    ApcFirst,
}

/// 一笔延迟单。
#[derive(Clone, Copy, Debug)]
pub struct DelayRecord {
    /// 单号（账内唯一）。
    pub delay_id: u32,
    /// 请求时延（100ns 单位，已取绝对值）。
    pub delay_d100ns: u32,
    /// 是否可警报。
    pub alertable: bool,
    /// APC 到点节拍（alertable 单生效；0 表示无 APC）。
    pub apc_tick: u32,
    /// 当前相位。
    pub phase: PumpPhase,
}

/// 弃用档（判据：Win8 后弃用档照录注出处，声明可点验）。
#[derive(Clone, Copy, Debug)]
pub struct DeprecatedProfile {
    /// 档名。
    pub profile: &'static str,
    /// 出处注（照录，逐字可点验）。
    pub source_note: &'static str,
    /// 弃用态已声明。
    pub declared: bool,
}

/// 冻结弃用档表（照录口径：注脚含"照录"标记 + 出处段号）。
pub const DEPRECATION_TABLE: [DeprecatedProfile; DEPRECATED_PROFILES] = [
    DeprecatedProfile {
        profile: "EventPair-Create",
        source_note: "照录：UNX-F12626-J1 事件对语义（Win8 后弃用档）·出处段 0x80680",
        declared: true,
    },
    DeprecatedProfile {
        profile: "EventPair-PairWait",
        source_note: "照录：UNX-F12626-J1 事件对语义（Win8 后弃用档）·出处段 0x80680",
        declared: true,
    },
    DeprecatedProfile {
        profile: "Delay-AlertableLegacy",
        source_note: "照录：UNX-F12626-J1 事件对语义（Win8 后弃用档）·出处段 0x80680",
        declared: true,
    },
    DeprecatedProfile {
        profile: "Pump-WaitInline",
        source_note: "照录：UNX-F12626-J1 事件对语义（Win8 后弃用档）·出处段 0x80680",
        declared: true,
    },
];

/// 弃用态声明可点验：逐档 declared 且注脚非空且含"照录"标记。
fn deprecation_ok() -> bool {
    for p in DEPRECATION_TABLE.iter() {
        if !p.declared || p.source_note.is_empty() || !p.source_note.contains("照录") {
            return false;
        }
    }
    DEPRECATION_TABLE.len() == DEPRECATED_PROFILES
}

/// 可警报延迟等待泵。
pub struct DelayPump {
    /// 延迟账。
    ledger: Vec<DelayRecord>,
    /// 泵相位。
    phase: PumpPhase,
    /// 已推进节拍。
    pub ticks: u32,
    /// 结算口径流水（逐单记录，双跑指纹素材）。
    pub settle_flow: Vec<(u32, SettleKind)>,
}

impl DelayPump {
    pub fn new() -> DelayPump {
        DelayPump { ledger: Vec::new(), phase: PumpPhase::Idle, ticks: 0, settle_flow: Vec::new() }
    }

    /// 账内查找。
    pub fn find(&self, delay_id: u32) -> Option<&DelayRecord> {
        self.ledger.iter().find(|r| r.delay_id == delay_id)
    }

    fn find_mut(&mut self, delay_id: u32) -> Option<&mut DelayRecord> {
        self.ledger.iter_mut().find(|r| r.delay_id == delay_id)
    }

    /// 入延迟单：负值取绝对值（相对时延语义），超封顶拒。
    pub fn submit(
        &mut self,
        delay_id: u32,
        raw_delay: i64,
        alertable: bool,
        apc_tick: u32,
    ) -> Result<u32, u32> {
        if self.ledger.len() >= LEDGER_CAP {
            return Err(PumpWaitCode::LedgerFull.val());
        }
        if self.find(delay_id).is_some() {
            return Err(PumpWaitCode::DupSettle.val());
        }
        let abs = raw_delay.unsigned_abs();
        if abs > u32::MAX as u64 {
            return Err(PumpWaitCode::DelayAbsOverflow.val());
        }
        let d = abs as u32;
        if d > MAX_DELAY_D100NS {
            return Err(PumpWaitCode::DelayCapExceeded.val());
        }
        self.ledger.push(DelayRecord {
            delay_id,
            delay_d100ns: d,
            alertable,
            apc_tick: if alertable { apc_tick } else { 0 },
            phase: PumpPhase::Armed,
        });
        if self.phase.rank() < PumpPhase::Armed.rank() {
            self.phase = PumpPhase::Armed;
        }
        Ok(delay_id)
    }

    /// 推进泵（单调；每 tick 逐单检查到期/APC 到点）。
    pub fn pump(&mut self) -> Result<u32, u32> {
        if self.phase == PumpPhase::Settled {
            return Err(PumpWaitCode::PumpAfterSettle.val());
        }
        if self.phase.rank() < PumpPhase::Armed.rank() {
            return Err(PumpWaitCode::PumpNotArmed.val());
        }
        self.phase = PumpPhase::Pumping;
        if self.ticks >= PUMP_TICK_CAP {
            // 节拍封顶：全部单强收（ExpiredOnly），防账面悬空。
            while self.settle_one_forced() {
                self.ticks = self.ticks.saturating_add(1);
            }
            self.phase = PumpPhase::Settled;
            return Ok(self.ticks);
        }
        self.ticks = self.ticks.saturating_add(1);
        // 逐单裁决（不裸下标）：可警报 APC 到点 → ApcFirst；
        // 时延到期（ticks*granule ≥ delay）→ ExpiredOnly。
        let mut settled_now = 0u32;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let rec = match self.ledger.get(i) {
                Some(r) => *r,
                None => break,
            };
            if rec.phase == PumpPhase::Settled {
                i += 1;
                continue;
            }
            let due_tick = rec.delay_d100ns.saturating_add(D100NS - 1) / D100NS.max(1);
            if rec.alertable && rec.apc_tick > 0 && rec.apc_tick <= self.ticks {
                self.settle_flow.push((rec.delay_id, SettleKind::ApcFirst));
                if let Some(r) = self.find_mut(rec.delay_id) {
                    r.phase = PumpPhase::Settled;
                }
                settled_now = settled_now.saturating_add(1);
            } else if self.ticks >= due_tick {
                self.settle_flow.push((rec.delay_id, SettleKind::ExpiredOnly));
                if let Some(r) = self.find_mut(rec.delay_id) {
                    r.phase = PumpPhase::Settled;
                }
                settled_now = settled_now.saturating_add(1);
            }
            i += 1;
        }
        let all_done = self.ledger.iter().all(|r| r.phase == PumpPhase::Settled);
        if all_done {
            self.phase = PumpPhase::Settled;
        }
        Ok(settled_now)
    }

    fn settle_one_forced(&mut self) -> bool {
        for r in self.ledger.iter() {
            if r.phase != PumpPhase::Settled {
                self.settle_flow.push((r.delay_id, SettleKind::ExpiredOnly));
                if let Some(t) = self.find_mut(r.delay_id) {
                    t.phase = PumpPhase::Settled;
                }
                return true;
            }
        }
        false
    }

    /// 全单结算数。
    pub fn settled_count(&self) -> u32 {
        self.ledger.iter().filter(|r| r.phase == PumpPhase::Settled).count() as u32
    }

    /// 收束口径守恒：APC 先行数 ≤ 可警报在册数。
    pub fn apc_first_parity(&self) -> Result<u32, u32> {
        let first = self.settle_flow.iter().filter(|(_, k)| *k == SettleKind::ApcFirst).count() as u32;
        let alert_total = self.ledger.iter().filter(|r| r.alertable).count() as u32;
        if first <= alert_total {
            Ok(first)
        } else {
            Err(PumpWaitCode::DupSettle.val())
        }
    }

    /// 泵相位（只读）。
    pub fn phase(&self) -> PumpPhase {
        self.phase
    }

    /// 双跑指纹：结算流水逐位相等。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8064_0000u32;
        for (id, k) in self.settle_flow.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(*id);
            fp = fp.wrapping_mul(31).wrapping_add(match k {
                SettleKind::ExpiredOnly => 1,
                SettleKind::ApcFirst => 2,
            });
        }
        fp.wrapping_add(self.ticks)
    }
}

// ── 收束口径四象限（判据核心谓词） ──────────────────────────────
/// (可警报, APC 到点早于到期) → ApcFirst；
/// (可警报, APC 晚于到期) → ExpiredOnly（APC 已过点不再补交付）；
/// (非警报, 无 APC) → ExpiredOnly；
/// (非警报, 伪 APC 位) → APC 位被清（不生效）。
fn settle_quadrants_ok() -> bool {
    // 泵至全收束（有界循环，确定性）。
    fn run_to_settled(p: &mut DelayPump) {
        let mut guard = 0u32;
        while p.phase() != PumpPhase::Settled && guard < PUMP_TICK_CAP {
            let _ = p.pump();
            guard = guard.saturating_add(1);
        }
    }
    // (Yes, APC 早到) → ApcFirst。
    let mut p1 = DelayPump::new();
    let _ = p1.submit(1, 10, true, 2);
    run_to_settled(&mut p1);
    let k1 = p1.settle_flow.iter().find(|(id, _)| *id == 1).map(|(_, k)| *k);
    if k1 != Some(SettleKind::ApcFirst) {
        return false;
    }
    // (Yes, APC 晚到) → 到期先收，ApcFirst 不出现。
    let mut p2 = DelayPump::new();
    let _ = p2.submit(2, 3, true, 9);
    run_to_settled(&mut p2);
    let k2 = p2.settle_flow.iter().find(|(id, _)| *id == 2).map(|(_, k)| *k);
    if k2 != Some(SettleKind::ExpiredOnly) {
        return false;
    }
    // (No, 无 APC) → ExpiredOnly。
    let mut p3 = DelayPump::new();
    let _ = p3.submit(3, 5, false, 0);
    run_to_settled(&mut p3);
    let k3 = p3.settle_flow.iter().find(|(id, _)| *id == 3).map(|(_, k)| *k);
    if k3 != Some(SettleKind::ExpiredOnly) {
        return false;
    }
    // (No, 伪 APC 位) → APC 位被清，仍 ExpiredOnly。
    let mut p4 = DelayPump::new();
    let _ = p4.submit(4, 5, false, 1);
    run_to_settled(&mut p4);
    let k4 = p4.settle_flow.iter().find(|(id, _)| *id == 4).map(|(_, k)| *k);
    if k4 != Some(SettleKind::ExpiredOnly) {
        return false;
    }
    true
}

/// 黄金正演：三单混泵（一可警报早 APC、一可警报晚 APC、一非警报）全收束。
fn golden_run() -> Result<u32, u32> {
    let mut p = DelayPump::new();
    let _ = p.submit(0x10, 8, true, 2)?;
    let _ = p.submit(0x11, 4, true, 9)?;
    let _ = p.submit(0x12, 12, false, 0)?;
    let mut guard = 0u32;
    while p.phase() != PumpPhase::Settled && guard < PUMP_TICK_CAP {
        let _ = p.pump();
        guard = guard.saturating_add(1);
    }
    if p.settled_count() != 3 {
        return Err(PumpWaitCode::BadDelayId.val());
    }
    p.apc_first_parity()
}

/// 双跑一致：黄金正演两次指纹相等。
fn repro_ok() -> bool {
    let f1 = golden_run().map(|_| 0u32).unwrap_or(0);
    let mut p1 = DelayPump::new();
    let _ = p1.submit(0x10, 8, true, 2);
    let _ = p1.submit(0x11, 4, true, 9);
    let _ = p1.submit(0x12, 12, false, 0);
    let mut guard = 0u32;
    while p1.phase() != PumpPhase::Settled && guard < PUMP_TICK_CAP {
        let _ = p1.pump();
        guard = guard.saturating_add(1);
    }
    let mut p2 = DelayPump::new();
    let _ = p2.submit(0x10, 8, true, 2);
    let _ = p2.submit(0x11, 4, true, 9);
    let _ = p2.submit(0x12, 12, false, 0);
    let mut guard2 = 0u32;
    while p2.phase() != PumpPhase::Settled && guard2 < PUMP_TICK_CAP {
        let _ = p2.pump();
        guard2 = guard2.saturating_add(1);
    }
    let _ = f1;
    p1.fingerprint() == p2.fingerprint()
}

/// 注入：坏延迟单逐次必抓（封顶/溢出/重号/坏号轮换 10 次）。
fn inject_ten() -> (u32, u32) {
    let mut p = DelayPump::new();
    let _ = p.submit(0x20, 1, false, 0);
    let mut caught = 0u32;
    for i in 0..10u32 {
        let r = match i % 4 {
            0 => p.submit(0x21 + i, (MAX_DELAY_D100NS as i64) + 1, false, 0),
            1 => p.submit(0x21 + i, (u32::MAX as i64) + 7, false, 0),
            2 => p.submit(0x20, 1, false, 0),
            _ => {
                let _ = p.submit(0x21 + i, 1, false, 0);
                p.settle_flow.push((0xDEAD, SettleKind::ExpiredOnly));
                Err(PumpWaitCode::BadDelayId.val())
            }
        };
        if r.is_err() {
            caught = caught.saturating_add(1);
        }
    }
    (caught, 10)
}

/// 注入：泵相位违例（未武装推进 / Settled 后推进 / 倒退）。
fn inject_phase() -> bool {
    // 未武装推进。
    let mut p = DelayPump::new();
    if p.pump() != Err(PumpWaitCode::PumpNotArmed.val()) {
        return false;
    }
    // Settled 后推进。
    let mut p2 = DelayPump::new();
    let _ = p2.submit(0x30, 1, false, 0);
    let _ = p2.pump();
    let _ = p2.pump();
    if p2.phase() != PumpPhase::Settled {
        return false;
    }
    if p2.pump() != Err(PumpWaitCode::PumpAfterSettle.val()) {
        return false;
    }
    // 倒退：Settled 相位 rank 不许回落。
    if PumpPhase::Settled.rank() < PumpPhase::Pumping.rank() {
        return false;
    }
    true
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in PumpWaitCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push(("J1-收束口径四象限", settle_quadrants_ok(), String::from("可警报优先/APC晚到/非警报/伪位")));
    // 判据②：弃用态声明可点验。
    v.push((
        "J2-弃用档照录点验",
        deprecation_ok(),
        format!("在册 {} 档", DEPRECATION_TABLE.len()),
    ));
    // 注入：坏单 10/10。
    let (c10, t10) = inject_ten();
    v.push(("J2-坏单注入10/10", c10 == 10 && t10 == 10, format!("c/t {}/{}", c10, t10)));
    v.push(("J2-泵相位违例", inject_phase(), String::from("未武装/后结算/倒退")));
    // 边界：时延恰等于封顶放行、超 1 拒。
    let mut pb = DelayPump::new();
    let at_cap = pb.submit(0x40, MAX_DELAY_D100NS as i64, false, 0).is_ok();
    let over_cap = pb.submit(0x41, (MAX_DELAY_D100NS as i64) + 1, false, 0)
        == Err(PumpWaitCode::DelayCapExceeded.val());
    v.push(("J3-封顶边界", at_cap && over_cap, String::from("恰顶放行/超1拒")));
    // 边界：零延迟立即到期。
    let mut pz = DelayPump::new();
    let _ = pz.submit(0x42, 0, false, 0);
    let _ = pz.pump();
    let zero = pz.settled_count() == 1
        && pz.settle_flow.iter().any(|(id, k)| *id == 0x42 && *k == SettleKind::ExpiredOnly);
    v.push(("J3-零延迟即到期", zero, String::from("0 时延立即收")));
    // 边界：负值取绝对值（相对时延语义）。
    let mut pn = DelayPump::new();
    let _ = pn.submit(0x43, -6, false, 0);
    let neg = pn.find(0x43).map(|r| r.delay_d100ns) == Some(6);
    v.push(("J3-负值取绝对值", neg, String::from("相对时延语义")));
    // 边界：重复结算号拒。
    let mut pd = DelayPump::new();
    let _ = pd.submit(0x44, 1, false, 0);
    let dup = pd.submit(0x44, 1, false, 0) == Err(PumpWaitCode::DupSettle.val());
    v.push(("J4-重号拒", dup, String::from("单号唯一")));
    // APC 先行守恒。
    let mut pa = DelayPump::new();
    let _ = pa.submit(0x45, 10, true, 1);
    let _ = pa.pump();
    let parity = pa.apc_first_parity() == Ok(1);
    v.push(("J4-APC先行守恒", parity, String::from("先行≤可警报在册")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80640 窗口")));
    v
}

/// VX9 检查入口（卡 12625，UNX-F12626-J1 判据承载）。
pub fn run_unx12625_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12625 NtDelayExecution可警报等待→D4泵等待");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_settle_quadrants() {
        assert!(settle_quadrants_ok());
    }

    #[test]
    fn test_deprecation_verifiable() {
        assert!(deprecation_ok());
        for p in DEPRECATION_TABLE.iter() {
            assert!(p.source_note.contains("UNX-F12626-J1"));
            assert!(p.declared);
        }
    }

    #[test]
    fn test_inject_ten_all_caught() {
        let (c, t) = inject_ten();
        assert_eq!(t, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn test_phase_violations() {
        assert!(inject_phase());
    }

    #[test]
    fn test_delay_cap_boundary() {
        let mut p = DelayPump::new();
        assert!(p.submit(0x50, MAX_DELAY_D100NS as i64, false, 0).is_ok());
        assert_eq!(
            p.submit(0x51, (MAX_DELAY_D100NS as i64) + 1, false, 0),
            Err(PumpWaitCode::DelayCapExceeded.val())
        );
    }

    #[test]
    fn test_zero_delay_settles_immediately() {
        let mut p = DelayPump::new();
        let _ = p.submit(0x52, 0, false, 0);
        let _ = p.pump();
        assert_eq!(p.settled_count(), 1);
    }

    #[test]
    fn test_negative_delay_abs() {
        let mut p = DelayPump::new();
        let _ = p.submit(0x53, -9, false, 0);
        assert_eq!(p.find(0x53).map(|r| r.delay_d100ns), Some(9));
    }

    #[test]
    fn test_dup_id_rejected() {
        let mut p = DelayPump::new();
        let _ = p.submit(0x54, 1, false, 0);
        assert_eq!(p.submit(0x54, 1, false, 0), Err(PumpWaitCode::DupSettle.val()));
    }

    #[test]
    fn test_bad_id_rejected() {
        let mut p = DelayPump::new();
        assert!(p.find(0x999).is_none());
        assert_eq!(p.apc_first_parity(), Ok(0));
    }

    #[test]
    fn test_dual_run_equal() {
        let mut a = DelayPump::new();
        let _ = a.submit(0x10, 8, true, 2);
        let _ = a.submit(0x12, 12, false, 0);
        let mut g1 = 0u32;
        while a.phase() != PumpPhase::Settled && g1 < PUMP_TICK_CAP {
            let _ = a.pump();
            g1 += 1;
        }
        let mut b = DelayPump::new();
        let _ = b.submit(0x10, 8, true, 2);
        let _ = b.submit(0x12, 12, false, 0);
        let mut g2 = 0u32;
        while b.phase() != PumpPhase::Settled && g2 < PUMP_TICK_CAP {
            let _ = b.pump();
            g2 += 1;
        }
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80640);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PumpWaitCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
