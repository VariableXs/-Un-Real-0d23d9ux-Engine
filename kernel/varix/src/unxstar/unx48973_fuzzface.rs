//! UNX-F48973 高频 fuzz（1,000Hz 满载 + 乱事件注入不崩不丢账）· 0x71580
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B09）：
//!   F48973 ① 纯功能代码 ≥260 行；② UNX-F48973-J1 fuzz ×10^4 零崩。
//!   F48974 ① 纯功能代码 ≥300 行；② UNX-F48974-J1 联轧 ×5 全绿零漂移。
//!   F48975 ① 纯功能代码 ≥220 行；② UNX-F48975-J1 错误码 ×15 零漂移互不混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：tick 单调 → delta 合法 → 整格拆分 → 恒等式复算 → 入账。
//! 先校 tick 再校 delta：tick 是事件的**时间轴身份**，若先按 delta 判合法，
//! 回退 tick 的乱事件会以「delta 合法」为由混进账本，丢帧检测随之失效。
//! 恒等式放在入账前：`总输入 = 整格×120 + 余量` 是唯一可观测守恒量；
//! 乱事件全部拒绝后，它仍是「已接受事件」的闭合式，账本才不是自说自话。
//! 关键差异：本模块只回答「1,000Hz 满载流在乱事件里是否不崩、不丢账、
//!           码族是否互不混淆」，不做 M8 风暴恢复（B09-STORM）、不做
//!           水位告警（B09-BACKPRESSURE）。谁把乱事件静默吞掉，谁就让
//!           「零崩」变成「零可观测」——拒绝必须显式计入 rejects。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71580，VX6 批 B09 族 fuzz 槽）。
pub const SEG_BASE: u32 = 0x71580;

/// 高报点率：1,000 Hz（1 ms 一帧）。
pub const POLL_HZ: u32 = 1_000;
/// 满载时长（分钟）。
pub const STREAM_MINUTES: u32 = 10;
/// 满载帧数：1,000 × 60 × 10 = 600,000。
pub const STREAM_TICKS: u32 = POLL_HZ * 60 * STREAM_MINUTES;
/// 探针帧数（自检快速档，仍满足零丢判据的抽样强度）。
pub const PROBE_TICKS: u32 = 60_000;
/// 乱事件 fuzz 轮数（卡面 ×10^4）。
pub const CHAOS_ROUNDS: u32 = 10_000;
/// 段内联轧轮数（卡面 ×5）。
pub const ROLL_ROUNDS: u32 = 5;
/// 联轧路数。
pub const ROUTES: usize = 5;
/// 错误码族规模（卡面 15 条）。
pub const CODE_FAMILY: usize = 15;
/// 乱事件类别数。
pub const CHAOS_KINDS: usize = 6;

/// FNV-1a 64 初始值（与 p_core 同参，跨模块可折叠）。
pub const FNV_OFF: u64 = p_core::FNV_OFF;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0F，恰 15 条） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HqCode {
    /// 流账不平（帧数/接受数/恒等式任一断裂）
    StreamBad,
    /// tick 非法（为零的哨兵值）
    TickBad,
    /// 次序非法（tick 回退或重复）
    OrderBad,
    /// delta 非法（空事件）
    DeltaBad,
    /// 越界（delta 超出输入层整数域）
    OverflowBad,
    /// 乱事件未被拒绝（注入缺陷穿透）
    ChaosBad,
    /// 账本不平（接受/拒绝合计对不上）
    LedgerBad,
    /// 摘要非法（零摘要或重放不一致）
    DigestBad,
    /// 路线非法（联轧路号越界）
    RouteBad,
    /// 覆盖不足（轮数/类别/码族项数不足）
    CoverBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 码族非法（15 条价值/次序破坏）
    CodeBad,
    /// 联轧失败（任一路红）
    RollBad,
    /// 复算不一致（同流两次结果不同）
    ReplayBad,
    /// 段位越界（诊断码逃出 0x71580 窗口）
    SegmentBad,
}

impl HqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HqCode::StreamBad => "流账不平：帧数/接受数/恒等式断裂",
            HqCode::TickBad => "tick 非法：零哨兵不得作为事件时间",
            HqCode::OrderBad => "次序非法：tick 回退或重复",
            HqCode::DeltaBad => "delta 非法：空事件不得入账",
            HqCode::OverflowBad => "越界：delta 超出输入层整数域",
            HqCode::ChaosBad => "乱事件未被拒绝：注入缺陷穿透",
            HqCode::LedgerBad => "账本不平：接受/拒绝合计对不上",
            HqCode::DigestBad => "摘要非法：零摘要或重放不一致",
            HqCode::RouteBad => "路线非法：联轧路号越界",
            HqCode::CoverBad => "覆盖不足：轮数/类别/码族项数不足",
            HqCode::RenderBad => "渲染非法：字段缺失或回读失败",
            HqCode::CodeBad => "码族非法：15 条价值或次序被破坏",
            HqCode::RollBad => "联轧失败：五路判据存在红项",
            HqCode::ReplayBad => "复算不一致：同流两次结果不同",
            HqCode::SegmentBad => "段位越界：诊断码逃出 0x71580 窗口",
        }
    }

    pub const ALL: [HqCode; CODE_FAMILY] = [
        HqCode::StreamBad,
        HqCode::TickBad,
        HqCode::OrderBad,
        HqCode::DeltaBad,
        HqCode::OverflowBad,
        HqCode::ChaosBad,
        HqCode::LedgerBad,
        HqCode::DigestBad,
        HqCode::RouteBad,
        HqCode::CoverBad,
        HqCode::RenderBad,
        HqCode::CodeBad,
        HqCode::RollBad,
        HqCode::ReplayBad,
        HqCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HqCode::ALL.len() {
        if HqCode::ALL[i].val() == code {
            return HqCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 1,000Hz 满载流 ────────────────────────────────────────────────
/// 满载流账：一帧一事件（严格单调 tick），余量线性携带。
///
/// 恒等式：`total = notches × 120 + remainder`，`remainder ∈ [-119, 119]`。
/// 注意余量是**线性携带**的：每帧 `remainder += delta % 120`，跨 120 才
/// 进位一格；把它当成「每帧独立取模」会漏掉跨帧进位（B08 首版实锤）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateStream {
    pub last_tick: Option<u32>,
    pub events: u64,
    pub accepts: u64,
    pub rejects: u64,
    pub notches: i64,
    pub remainder: i32,
    pub total: i64,
    pub digest: u64,
}

impl RateStream {
    pub fn new() -> RateStream {
        RateStream {
            last_tick: None,
            events: 0,
            accepts: 0,
            rejects: 0,
            notches: 0,
            remainder: 0,
            total: 0,
            digest: FNV_OFF,
        }
    }

    fn reject(&mut self, code: u32) -> Result<(), u32> {
        self.rejects = self.rejects.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 单帧入账。拒绝时**不推进** last_tick，也不改 total/notches。
    pub fn push(&mut self, tick: u32, delta: i32) -> Result<(), u32> {
        if tick == 0 {
            return self.reject(HqCode::TickBad.val());
        }
        if let Some(prev) = self.last_tick {
            // 满载流一帧一事件：相等 tick 表示重复投递，必须拒绝而不是合并。
            if tick <= prev {
                return self.reject(HqCode::OrderBad.val());
            }
        }
        if delta == 0 {
            return self.reject(HqCode::DeltaBad.val());
        }
        if (delta as i64).unsigned_abs() > p_core::DELTA_MAX as u64 {
            return self.reject(HqCode::OverflowBad.val());
        }
        let (units, part) = match p_core::wheel_units(delta) {
            Ok(v) => v,
            Err(_) => return self.reject(HqCode::DeltaBad.val()),
        };
        let total = match self.total.checked_add(delta as i64) {
            Some(v) => v,
            None => return self.reject(HqCode::OverflowBad.val()),
        };
        let sum = self.remainder as i64 + part as i64;
        let carry = sum / p_core::WHEEL_DELTA as i64;
        let new_rem = sum % p_core::WHEEL_DELTA as i64;
        if new_rem.unsigned_abs() > (p_core::WHEEL_DELTA - 1) as u64 {
            return self.reject(HqCode::LedgerBad.val());
        }
        let add = units as i64 + carry;
        let notches = match self.notches.checked_add(add) {
            Some(v) => v,
            None => return self.reject(HqCode::OverflowBad.val()),
        };
        self.total = total;
        self.notches = notches;
        self.remainder = new_rem as i32;
        self.events = self.events.saturating_add(1);
        self.accepts = self.accepts.saturating_add(1);
        self.last_tick = Some(tick);
        self.digest = p_core::fold_u32(self.digest, tick);
        self.digest = p_core::fold_i32(self.digest, delta);
        self.digest = p_core::fold_u32(self.digest, add as u32);
        Ok(())
    }

    /// 满载流对账：一帧一事件、恒等式、余量界三重闭合。
    pub fn audit(&self, ticks: u32) -> Result<(), u32> {
        if ticks == 0 {
            return Err(HqCode::CoverBad.val());
        }
        if self.events != ticks as u64 {
            return Err(HqCode::StreamBad.val());
        }
        if self.accepts != self.events {
            return Err(HqCode::LedgerBad.val());
        }
        if self.remainder.unsigned_abs() > (p_core::WHEEL_DELTA - 1) as u32 {
            return Err(HqCode::LedgerBad.val());
        }
        let recomposed = match self.notches.checked_mul(p_core::WHEEL_DELTA as i64) {
            Some(v) => v,
            None => return Err(HqCode::LedgerBad.val()),
        };
        let recomposed = match recomposed.checked_add(self.remainder as i64) {
            Some(v) => v,
            None => return Err(HqCode::LedgerBad.val()),
        };
        if recomposed != self.total {
            return Err(HqCode::StreamBad.val());
        }
        Ok(())
    }
}

/// 满载流合成：tick 从 1 起严格递增，delta 落在 [120, 240]（整格附近）。
pub fn stream_rounds(ticks: u32) -> Result<RateStream, u32> {
    if ticks == 0 {
        return Err(HqCode::CoverBad.val());
    }
    let mut st = RateStream::new();
    let mut rng = p_core::Lcg::new(0x8A11_0073);
    let mut i = 0u32;
    while i < ticks {
        let span = rng.below(121);
        let delta = p_core::WHEEL_DELTA + span as i32;
        st.push(i.saturating_add(1), delta)?;
        i = i.saturating_add(1);
    }
    st.audit(ticks)?;
    Ok(st)
}

// ── 乱事件注入 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChaosKind {
    /// 合法样本（单调 tick + 合法 delta）
    Valid,
    /// 空事件（delta = 0）
    Zero,
    /// 越界（delta = DELTA_MAX + 1）
    Oversize,
    /// tick 回退（等于上一帧之后又退回）
    BackTick,
    /// tick 重复（重复投递同一旧帧）
    DupTick,
    /// tick 零哨兵
    ZeroTick,
}

impl ChaosKind {
    pub fn index(self) -> usize {
        match self {
            ChaosKind::Valid => 0,
            ChaosKind::Zero => 1,
            ChaosKind::Oversize => 2,
            ChaosKind::BackTick => 3,
            ChaosKind::DupTick => 4,
            ChaosKind::ZeroTick => 5,
        }
    }

    pub fn from_index(i: usize) -> Result<ChaosKind, u32> {
        match i {
            0 => Ok(ChaosKind::Valid),
            1 => Ok(ChaosKind::Zero),
            2 => Ok(ChaosKind::Oversize),
            3 => Ok(ChaosKind::BackTick),
            4 => Ok(ChaosKind::DupTick),
            5 => Ok(ChaosKind::ZeroTick),
            _ => Err(HqCode::ChaosBad.val()),
        }
    }

    pub fn expected_ok(self) -> bool {
        self == ChaosKind::Valid
    }

    pub const ALL: [ChaosKind; CHAOS_KINDS] = [
        ChaosKind::Valid,
        ChaosKind::Zero,
        ChaosKind::Oversize,
        ChaosKind::BackTick,
        ChaosKind::DupTick,
        ChaosKind::ZeroTick,
    ];
}

/// 乱事件账：流账 + 类别分布 + 处置错配计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChaosLedger {
    pub stream: RateStream,
    pub attempts: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub faults: u64,
    pub kinds: [u64; CHAOS_KINDS],
    pub digest: u64,
}

impl ChaosLedger {
    pub fn new() -> ChaosLedger {
        ChaosLedger {
            stream: RateStream::new(),
            attempts: 0,
            accepted: 0,
            rejected: 0,
            faults: 0,
            kinds: [0; CHAOS_KINDS],
            digest: FNV_OFF,
        }
    }

    /// 记一笔乱事件。处置错配由调用方复算后累加 faults，这里只如实分类。
    pub fn admit(&mut self, kind: ChaosKind, tick: u32, delta: i32) -> Result<(), u32> {
        let idx = kind.index();
        if let Some(c) = self.kinds.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.attempts = self.attempts.saturating_add(1);
        let res = self.stream.push(tick, delta);
        if res.is_ok() {
            self.accepted = self.accepted.saturating_add(1);
        } else {
            self.rejected = self.rejected.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, idx as u32);
        self.digest = p_core::fold_u32(self.digest, tick);
        self.digest = p_core::fold_i32(self.digest, delta);
        res
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.attempts == 0 {
            return Err(HqCode::CoverBad.val());
        }
        let sum = self.accepted.saturating_add(self.rejected);
        if sum != self.attempts {
            return Err(HqCode::LedgerBad.val());
        }
        if self.faults != 0 {
            return Err(HqCode::ChaosBad.val());
        }
        if self.accepted == 0 || self.rejected == 0 {
            return Err(HqCode::CoverBad.val());
        }
        let mut i = 0usize;
        while i < CHAOS_KINDS {
            match self.kinds.get(i) {
                Some(c) => {
                    if *c == 0 {
                        return Err(HqCode::CoverBad.val());
                    }
                }
                None => return Err(HqCode::CoverBad.val()),
            }
            i += 1;
        }
        Ok(())
    }
}

/// 乱事件 fuzz：合法与五类非法按高位路由轮转，任何类别都不得穿透。
pub fn chaos_rounds(rounds: u32) -> Result<ChaosLedger, u32> {
    if rounds == 0 {
        return Err(HqCode::CoverBad.val());
    }
    let mut led = ChaosLedger::new();
    // 预置一帧合法基线，使「回退/重复」有确定的上一帧可退。
    let seed = led.stream.push(1, p_core::WHEEL_DELTA);
    if seed.is_err() || led.stream.events != 1 {
        return Err(HqCode::StreamBad.val());
    }
    let mut rng = p_core::Lcg::new(0x8A11_0074);
    let mut tick = 2u32;
    let mut i = 0u32;
    while i < rounds {
        // 路由取高位：LCG 低 3 位周期 8，与类别数 6 同步会锁死单类。
        let route = ((rng.next() >> 33) % ChaosKind::ALL.len() as u64) as usize;
        let kind = ChaosKind::from_index(route)?;
        let (t, d) = match kind {
            ChaosKind::Valid => {
                let span = rng.below(240);
                (tick, p_core::WHEEL_DELTA + span as i32)
            }
            ChaosKind::Zero => (tick, 0),
            ChaosKind::Oversize => (tick, p_core::DELTA_MAX.saturating_add(1)),
            ChaosKind::BackTick => (tick.saturating_sub(1).max(1), p_core::WHEEL_DELTA),
            ChaosKind::DupTick => (tick.saturating_sub(2).max(1), -p_core::WHEEL_DELTA),
            ChaosKind::ZeroTick => (0, p_core::WHEEL_DELTA),
        };
        let res = led.admit(kind, t, d);
        if res.is_ok() != kind.expected_ok() {
            led.faults = led.faults.saturating_add(1);
        }
        if kind == ChaosKind::Valid && res.is_ok() {
            tick = tick.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    led.audit()?;
    Ok(led)
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_ledger(st: &RateStream) -> String {
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        st.events, st.notches, st.remainder, st.accepts, st.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HqCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HqCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HqCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HqCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_i32(s: &str) -> Result<i32, u32> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let mag = parse_u32(body)?;
    if mag > i32::MAX as u32 {
        return Err(HqCode::RenderBad.val());
    }
    if neg {
        Ok(-(mag as i32))
    } else {
        Ok(mag as i32)
    }
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HqCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HqCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HqCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HqCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 回读：字段数、余量界、接受数上界、摘要非零四处校验，返回帧数。
pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(HqCode::RenderBad.val());
    }
    let events = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(HqCode::RenderBad.val()),
    })?;
    let notches = parse_i32(match parts.get(1) {
        Some(v) => v,
        None => return Err(HqCode::RenderBad.val()),
    })?;
    let rem = parse_i32(match parts.get(2) {
        Some(v) => v,
        None => return Err(HqCode::RenderBad.val()),
    })?;
    let accepts = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(HqCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(HqCode::RenderBad.val()),
    })?;
    if events == 0 {
        return Err(HqCode::RenderBad.val());
    }
    if accepts != events {
        return Err(HqCode::StreamBad.val());
    }
    if rem.unsigned_abs() > (p_core::WHEEL_DELTA - 1) as u32 {
        return Err(HqCode::LedgerBad.val());
    }
    let recomposed = match (notches as i64)
        .checked_mul(p_core::WHEEL_DELTA as i64)
        .and_then(|v| v.checked_add(rem as i64))
    {
        Some(v) => v,
        None => return Err(HqCode::LedgerBad.val()),
    };
    if recomposed == 0 && notches != 0 {
        return Err(HqCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(HqCode::RenderBad.val());
    }
    Ok(events)
}

pub fn golden_run() -> Result<(u32, u64, u64, u64), u32> {
    let st = stream_rounds(PROBE_TICKS)?;
    let text = render_ledger(&st);
    let events = audit_render(&text)?;
    if events != st.events as u32 {
        return Err(HqCode::ReplayBad.val());
    }
    let ch = chaos_rounds(1_000)?;
    Ok((events, ch.accepted, ch.rejected, st.digest ^ ch.digest))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(HqCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        let mut st = RateStream::new();
        let _ = st.push(5, p_core::WHEEL_DELTA);
        // ① tick 回退必须炸 OrderBad。
        if st.push(5, p_core::WHEEL_DELTA).err() == Some(HqCode::OrderBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 空事件必须炸 DeltaBad。
        if st.push(6, 0).err() == Some(HqCode::DeltaBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 越界必须炸 OverflowBad。
        if st.push(6, p_core::DELTA_MAX.saturating_add(1)).err()
            == Some(HqCode::OverflowBad.val())
        {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let text = render_ledger(&st).replacen("0x", "0y", 1);
        if audit_render(&text).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(HqCode::RollBad.val());
    }
    Ok(log)
}

// ── 段内五路联轧 ──────────────────────────────────────────────────
/// 五路：满载流、乱事件、账本回读、复跑一致、码族闭集。
pub fn roll_route(route: usize) -> Result<bool, u32> {
    match route {
        0 => {
            let st = stream_rounds(2_000)?;
            Ok(st.audit(2_000).is_ok() && st.events == 2_000)
        }
        1 => {
            let ch = chaos_rounds(500)?;
            Ok(ch.audit().is_ok())
        }
        2 => {
            let st = stream_rounds(500)?;
            Ok(audit_render(&render_ledger(&st)) == Ok(500))
        }
        3 => Ok(repro_check().is_ok()),
        4 => Ok(codes_are_distinct() && segment_guard()),
        _ => Err(HqCode::RouteBad.val()),
    }
}

pub fn roll_rounds(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 {
        return Err(HqCode::CoverBad.val());
    }
    let mut i = 0u32;
    while i < rounds {
        let mut r = 0usize;
        while r < ROUTES {
            if !roll_route(r)? {
                return Err(HqCode::RollBad.val());
            }
            r += 1;
        }
        i = i.saturating_add(1);
    }
    Ok(rounds)
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    if HqCode::ALL.len() != CODE_FAMILY {
        return false;
    }
    let mut i = 0usize;
    while i < HqCode::ALL.len() {
        let mut j = i + 1;
        while j < HqCode::ALL.len() {
            if HqCode::ALL[i].val() == HqCode::ALL[j].val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HqCode::ALL.len() {
        let v = HqCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let st = stream_rounds(PROBE_TICKS)?;
    if st.events != PROBE_TICKS as u64 || st.accepts != st.events {
        return Err(HqCode::StreamBad.val());
    }
    if st.remainder.unsigned_abs() > (p_core::WHEEL_DELTA - 1) as u32 {
        return Err(HqCode::LedgerBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let ch = chaos_rounds(2_000)?;
    if ch.audit().is_err() {
        return Err(HqCode::ChaosBad.val());
    }
    if ch.accepted == 0 || ch.rejected == 0 {
        return Err(HqCode::CoverBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if !codes_are_distinct() {
        return Err(HqCode::CodeBad.val());
    }
    if !segment_guard() {
        return Err(HqCode::SegmentBad.val());
    }
    let st = stream_rounds(1_000)?;
    if audit_render(&render_ledger(&st)) != Ok(1_000) {
        return Err(HqCode::RenderBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = stream_rounds(1_000)?;
    let b = stream_rounds(1_000)?;
    if a.digest != b.digest || a.events != b.events {
        return Err(HqCode::ReplayBad.val());
    }
    let c = chaos_rounds(300)?;
    let d = chaos_rounds(300)?;
    if c.digest != d.digest || c.kinds != d.kinds {
        return Err(HqCode::ReplayBad.val());
    }
    Ok(())
}

// ── 域检查项（动态明细槽上限 8，超槽即 truncated） ────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let full = stream_rounds(STREAM_TICKS);
    out.push((
        "F48973 1000Hz×10min零丢",
        matches!(&full, Ok(st) if st.events == STREAM_TICKS as u64
            && st.accepts == st.events && st.audit(STREAM_TICKS).is_ok()),
        match &full {
            Ok(st) => format!(
                "ticks={} events={} notches={} rem={}",
                STREAM_TICKS, st.events, st.notches, st.remainder
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let chaos = chaos_rounds(CHAOS_ROUNDS);
    out.push((
        "F48973 乱事件×10^4零崩",
        matches!(&chaos, Ok(ch) if ch.audit().is_ok() && ch.faults == 0),
        match &chaos {
            Ok(ch) => format!(
                "attempts={} ok={} bad={} faults={}",
                ch.attempts, ch.accepted, ch.rejected, ch.faults
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let roll = roll_rounds(ROLL_ROUNDS);
    out.push((
        "F48974 五路联轧×5全绿",
        roll == Ok(ROLL_ROUNDS),
        match &roll {
            Ok(n) => format!("routes={} rounds={}", ROUTES, n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48975 错误码×15零混淆",
        codes_are_distinct() && HqCode::ALL.len() == CODE_FAMILY,
        format!("codes={} family={}", HqCode::ALL.len(), CODE_FAMILY),
    ));

    let text = stream_rounds(1_000).map(|st| render_ledger(&st));
    out.push((
        "F48973 账本可解析",
        matches!(&text, Ok(t) if audit_render(t) == Ok(1_000)),
        match &text {
            Ok(t) => format!("text={}", t),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48973 复跑同指纹",
        repro_check().is_ok(),
        format!("repro={:?}", repro_check()),
    ));

    out.push((
        "F48973 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", HqCode::ALL.len(), SEG_BASE),
    ));

    let inj = inject_rounds(5);
    out.push((
        "F48973 注入四路全捕",
        matches!(&inj, Ok(l) if l.all_caught()),
        match &inj {
            Ok(l) => format!(
                "tries={:?} caught={:?}",
                l.tries, l.caught
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48973_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48973 高频 fuzz 与段内联轧");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_consistent() {
        assert_eq!(POLL_HZ, 1_000);
        assert_eq!(STREAM_TICKS, 600_000);
        assert_eq!(STREAM_TICKS, POLL_HZ * 60 * STREAM_MINUTES);
        assert!(PROBE_TICKS < STREAM_TICKS);
        assert_eq!(HqCode::ALL.len(), CODE_FAMILY);
    }

    #[test]
    fn stream_zero_loss_probe() {
        match stream_rounds(PROBE_TICKS) {
            Ok(st) => {
                assert_eq!(st.events, PROBE_TICKS as u64);
                assert_eq!(st.accepts, st.events);
                assert_eq!(st.rejects, 0);
                assert!(st.audit(PROBE_TICKS).is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn stream_full_ten_minutes() {
        match stream_rounds(STREAM_TICKS) {
            Ok(st) => {
                assert_eq!(st.events, 600_000);
                assert_eq!(st.accepts, 600_000);
                assert!(st.audit(STREAM_TICKS).is_ok());
                assert!(st.remainder.unsigned_abs() < 120);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn stream_rejects_zero_delta() {
        let mut st = RateStream::new();
        assert_eq!(st.push(1, 0), Err(HqCode::DeltaBad.val()));
        assert_eq!(st.rejects, 1);
        assert_eq!(st.events, 0);
    }

    #[test]
    fn stream_rejects_oversize() {
        let mut st = RateStream::new();
        assert_eq!(
            st.push(1, p_core::DELTA_MAX + 1),
            Err(HqCode::OverflowBad.val())
        );
        assert_eq!(st.rejects, 1);
    }

    #[test]
    fn stream_rejects_back_tick() {
        let mut st = RateStream::new();
        assert_eq!(st.push(5, 120), Ok(()));
        assert_eq!(st.push(4, 120), Err(HqCode::OrderBad.val()));
        assert_eq!(st.push(5, 120), Err(HqCode::OrderBad.val()));
        assert_eq!(st.rejects, 2);
    }

    #[test]
    fn stream_rejects_zero_tick() {
        let mut st = RateStream::new();
        assert_eq!(st.push(0, 120), Err(HqCode::TickBad.val()));
    }

    #[test]
    fn stream_carries_remainder_linearly() {
        let mut st = RateStream::new();
        assert_eq!(st.push(1, 100), Ok(()));
        assert_eq!(st.remainder, 100);
        assert_eq!(st.push(2, 80), Ok(()));
        assert_eq!(st.notches, 1);
        assert_eq!(st.remainder, 60);
        assert_eq!(st.total, 180);
    }

    #[test]
    fn stream_negative_is_symmetric() {
        let mut st = RateStream::new();
        assert_eq!(st.push(1, -100), Ok(()));
        assert_eq!(st.remainder, -100);
        assert_eq!(st.push(2, -80), Ok(()));
        assert_eq!(st.notches, -1);
        assert_eq!(st.remainder, -60);
        assert!(st.audit(2).is_ok());
    }

    #[test]
    fn chaos_balance_and_no_fault() {
        match chaos_rounds(1_000) {
            Ok(ch) => {
                assert_eq!(ch.attempts, ch.accepted + ch.rejected);
                assert_eq!(ch.faults, 0);
                assert!(ch.accepted > 0);
                assert!(ch.rejected > 0);
                assert!(ch.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn chaos_covers_all_kinds() {
        match chaos_rounds(CHAOS_ROUNDS) {
            Ok(ch) => {
                let mut i = 0usize;
                while i < CHAOS_KINDS {
                    assert!(ch.kinds[i] > 0, "kind {}", i);
                    i += 1;
                }
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn chaos_rejects_each_invalid_kind() {
        let mut led = ChaosLedger::new();
        assert_eq!(led.admit(ChaosKind::Valid, 1, 120), Ok(()));
        assert_eq!(
            led.admit(ChaosKind::Zero, 2, 0),
            Err(HqCode::DeltaBad.val())
        );
        assert_eq!(
            led.admit(ChaosKind::Oversize, 2, p_core::DELTA_MAX + 1),
            Err(HqCode::OverflowBad.val())
        );
        assert_eq!(
            led.admit(ChaosKind::BackTick, 1, 120),
            Err(HqCode::OrderBad.val())
        );
        assert_eq!(
            led.admit(ChaosKind::DupTick, 1, 120),
            Err(HqCode::OrderBad.val())
        );
        assert_eq!(
            led.admit(ChaosKind::ZeroTick, 0, 120),
            Err(HqCode::TickBad.val())
        );
    }

    #[test]
    fn chaos_kind_index_roundtrip() {
        let mut i = 0usize;
        while i < ChaosKind::ALL.len() {
            let k = ChaosKind::ALL[i];
            assert_eq!(ChaosKind::from_index(k.index()), Ok(k));
            assert_eq!(k.expected_ok(), k == ChaosKind::Valid);
            i += 1;
        }
        assert_eq!(
            ChaosKind::from_index(CHAOS_KINDS),
            Err(HqCode::ChaosBad.val())
        );
    }

    #[test]
    fn codes_are_fifteen_distinct() {
        assert_eq!(HqCode::ALL.len(), 15);
        assert!(codes_are_distinct());
        assert!(!detail(HqCode::StreamBad.val()).is_empty());
    }

    #[test]
    fn codes_fit_window() {
        let mut i = 0usize;
        while i < HqCode::ALL.len() {
            let v = HqCode::ALL[i].val();
            assert!(v > SEG_BASE);
            assert!(v <= SEG_BASE + 0x0F);
            i += 1;
        }
        assert!(segment_guard());
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let st = stream_rounds(1_000).unwrap_or_else(|_| RateStream::new());
        let text = render_ledger(&st);
        assert_eq!(audit_render(&text), Ok(1_000));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        assert!(audit_render("1|2|3").is_err());
    }

    #[test]
    fn render_rejects_inconsistent_accepts() {
        let st = stream_rounds(10).unwrap_or_else(|_| RateStream::new());
        let text = render_ledger(&st);
        let mut parts: Vec<&str> = text.split('|').collect();
        if let Some(p) = parts.get_mut(3) {
            *p = "9999";
        }
        let bad = parts.join("|");
        assert_eq!(audit_render(&bad), Err(HqCode::StreamBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn roll_five_routes_green() {
        assert_eq!(roll_rounds(ROLL_ROUNDS), Ok(ROLL_ROUNDS));
        assert_eq!(roll_route(ROUTES), Err(HqCode::RouteBad.val()));
        assert_eq!(roll_rounds(0), Err(HqCode::CoverBad.val()));
    }

    #[test]
    fn golden_is_stable() {
        match golden_run() {
            Ok((events, ok, bad, d)) => {
                assert_eq!(events, PROBE_TICKS);
                assert!(ok > 0);
                assert!(bad > 0);
                assert!(d != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48973_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
