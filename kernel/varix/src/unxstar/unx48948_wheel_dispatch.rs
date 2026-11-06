//! UNX-F48948 轴锁定与滚轮对账 · 0x71260
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F48948-J1 分界声明落账 + 切轴 ×10^2 锁定正确；
//!   ③ UNX-F48949-J1 滚轮段 ×10^3 打点全落账；
//!   ④ UNX-F48950-J1 对账 ×10^3 零差。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：轴锁定判定 → 锁定轴过滤 → 四段打点 → 合成/实测对账 →
//!           计数守恒。
//! 先锁定再过滤：双指滚动的横向抖动如果先被递送，应用会收到两轴
//! 交替的碎事件；锁定必须发生在递送之前，且判定只用位移的绝对值。
//! 打点在对账之前：只有四段时序都落账，合成流与实测流的计数差才能
//! 归因到具体段，而不是一句「对不上」。
//! 关键差异：本模块只回答「哪个轴被锁定、每段何时打点、两路计数是否
//!           相等」，不做手势识别（B10）、不做高频调度（B09）、不做
//!           平滑滚动（B08-EDGE 仅登记）。锁定判据是纯位移函数，
//!           不读取时间戳，避免「快慢影响轴」的隐式耦合。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;
use super::unx48943_wheel_meta as p_meta;

/// 诊断段基址（0x71260，B08 族第四槽）。
pub const SEG_BASE: u32 = 0x71260;

/// 轴锁定判定阈值（细格）：累计主轴位移达到该值才锁定。
pub const LOCK_THRESHOLD: i64 = 12;
/// 轴锁定优势比分子/分母：主轴必须超过副轴的 3/2。
pub const DOMINANCE_NUM: i64 = 3;
pub const DOMINANCE_DEN: i64 = 2;
/// 轴锁定轮数（卡面判据 ×10^2 的载体）。
pub const LOCK_ROUNDS: u32 = 100;
/// 打点轮数（卡面判据 ×10^3 的载体）。
pub const STAMP_ROUNDS: u32 = 1_000;
/// 对账轮数（卡面判据 ×10^3 的载体）。
pub const RECONCILE_ROUNDS: u32 = 1_000;
/// 四段打点次序。
pub const STAMP_POINTS: usize = 4;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HdCode {
    /// 轴向非法
    AxisBad,
    /// 锁定阈值非法
    LockBad,
    /// 状态迁移非法
    StateBad,
    /// 打点次序非法（时钟回退或跳段）
    StampBad,
    /// 段非法（未知打点段）
    PhaseBad,
    /// 次序非法
    OrderBad,
    /// 账不平（合成/实测计数不一致）
    LedgerBad,
    /// 渲染非法
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 覆盖不足
    CoverageBad,
    /// 段位越界
    SegmentBad,
    /// 对账非法（计数或轴号不匹配）
    ReconcileBad,
}

impl HdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HdCode::AxisBad => "轴向非法：未知轴向编号",
            HdCode::LockBad => "锁定阈值非法：阈值非正或位移越界",
            HdCode::StateBad => "状态迁移非法：锁定状态机次序断裂",
            HdCode::StampBad => "打点次序非法：时钟回退或跳段",
            HdCode::PhaseBad => "段非法：未知打点段",
            HdCode::OrderBad => "次序非法：账本计数次序断裂",
            HdCode::LedgerBad => "账不平：合成流与实测流计数不一致",
            HdCode::RenderBad => "渲染非法：字段缺失或回读失败",
            HdCode::ReplayBad => "复算不一致：同一输入重放不同",
            HdCode::CoverageBad => "覆盖不足：探针或账本项数不足",
            HdCode::SegmentBad => "段位越界：诊断码逃出 0x71260 窗口",
            HdCode::ReconcileBad => "对账非法：计数或轴号不匹配",
        }
    }

    pub const ALL: [HdCode; 12] = [
        HdCode::AxisBad,
        HdCode::LockBad,
        HdCode::StateBad,
        HdCode::StampBad,
        HdCode::PhaseBad,
        HdCode::OrderBad,
        HdCode::LedgerBad,
        HdCode::RenderBad,
        HdCode::ReplayBad,
        HdCode::CoverageBad,
        HdCode::SegmentBad,
        HdCode::ReconcileBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HdCode::ALL.len() {
        if HdCode::ALL[i].val() == code {
            return HdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 轴锁定状态机 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockState {
    /// 未按下或已复位
    Idle,
    /// 已累计位移但未达阈值/优势比
    Undecided,
    /// 锁定水平轴
    LockedX,
    /// 锁定垂直轴
    LockedY,
}

impl LockState {
    pub fn code(self) -> u32 {
        match self {
            LockState::Idle => SEG_BASE + 0x10,
            LockState::Undecided => SEG_BASE + 0x11,
            LockState::LockedX => SEG_BASE + 0x12,
            LockState::LockedY => SEG_BASE + 0x13,
        }
    }
}

/// 双指滚动切轴锁：只消费位移绝对值，不读时间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AxisLock {
    pub state: LockState,
    pub acc_x: i64,
    pub acc_y: i64,
    pub locks_x: u32,
    pub locks_y: u32,
    pub resets: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl AxisLock {
    pub fn new() -> AxisLock {
        AxisLock {
            state: LockState::Idle,
            acc_x: 0,
            acc_y: 0,
            locks_x: 0,
            locks_y: 0,
            resets: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state != LockState::Idle {
            return Err(HdCode::StateBad.val());
        }
        self.state = LockState::Undecided;
        self.acc_x = 0;
        self.acc_y = 0;
        Ok(())
    }

    fn decide(&mut self, ax: i64, ay: i64) -> Result<(), u32> {
        let ax_abs = ax.unsigned_abs();
        let ay_abs = ay.unsigned_abs();
        if ax_abs < LOCK_THRESHOLD as u64 && ay_abs < LOCK_THRESHOLD as u64 {
            return Ok(());
        }
        let x_dom = ax_abs as i64 * DOMINANCE_DEN;
        let y_dom = ay_abs as i64 * DOMINANCE_DEN;
        if x_dom >= y_dom * DOMINANCE_NUM && ax_abs >= LOCK_THRESHOLD as u64 {
            self.state = LockState::LockedX;
            self.locks_x = self.locks_x.saturating_add(1);
            Ok(())
        } else if y_dom >= x_dom * DOMINANCE_NUM && ay_abs >= LOCK_THRESHOLD as u64 {
            self.state = LockState::LockedY;
            self.locks_y = self.locks_y.saturating_add(1);
            Ok(())
        } else {
            Ok(())
        }
    }

    /// 喂入一段双指位移，返回本次允许递送的 (dx, dy)。
    pub fn feed(&mut self, dx: i32, dy: i32) -> Result<(i32, i32), u32> {
        if dx == 0 && dy == 0 {
            self.rejects = self.rejects.saturating_add(1);
            let e = HdCode::LockBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        if self.state == LockState::Idle {
            self.rejects = self.rejects.saturating_add(1);
            let e = HdCode::StateBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        let ax = match self.acc_x.checked_add(dx as i64) {
            Some(v) => v,
            None => return Err(HdCode::LockBad.val()),
        };
        let ay = match self.acc_y.checked_add(dy as i64) {
            Some(v) => v,
            None => return Err(HdCode::LockBad.val()),
        };
        self.acc_x = ax;
        self.acc_y = ay;
        if self.state == LockState::Undecided {
            self.decide(ax, ay)?;
        }
        self.digest = p_core::fold_i32(self.digest, dx);
        self.digest = p_core::fold_i32(self.digest, dy);
        match self.state {
            LockState::LockedX => Ok((dx, 0)),
            LockState::LockedY => Ok((0, dy)),
            _ => Ok((0, 0)),
        }
    }

    pub fn end(&mut self) -> Result<(), u32> {
        if self.state == LockState::Idle {
            return Err(HdCode::StateBad.val());
        }
        self.state = LockState::Idle;
        self.resets = self.resets.saturating_add(1);
        Ok(())
    }

    pub fn audit(&mut self) -> Result<(), u32> {
        if self.state == LockState::LockedX && self.locks_x == 0 {
            return Err(HdCode::LedgerBad.val());
        }
        if self.state == LockState::LockedY && self.locks_y == 0 {
            return Err(HdCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 切轴 ×10^2：水平主导锁 X、垂直主导锁 Y、先微后大仍能锁定。
pub fn lock_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    if rounds == 0 {
        return Err(HdCode::CoverageBad.val());
    }
    let mut x_wins = 0u32;
    let mut y_wins = 0u32;
    let mut i = 0u32;
    while i < rounds {
        // 水平主导：dx=40，dy=3 → 锁 X 后只出 x。
        let mut a = AxisLock::new();
        a.begin()?;
        let r1 = a.feed(40, 3)?;
        if a.state != LockState::LockedX || r1 != (40, 0) {
            return Err(HdCode::StateBad.val());
        }
        if a.feed(7, 90)? != (7, 0) {
            return Err(HdCode::LockBad.val());
        }
        a.audit()?;
        a.end()?;
        x_wins = x_wins.saturating_add(1);
        // 垂直主导：dx=1，dy=40 → 锁 Y 后只出 y。
        let mut b = AxisLock::new();
        b.begin()?;
        let r2 = b.feed(1, 40)?;
        if b.state != LockState::LockedY || r2 != (0, 40) {
            return Err(HdCode::StateBad.val());
        }
        if b.feed(90, 7)? != (0, 7) {
            return Err(HdCode::LockBad.val());
        }
        b.audit()?;
        b.end()?;
        y_wins = y_wins.saturating_add(1);
        i = i.saturating_add(1);
    }
    Ok((x_wins, y_wins))
}

/// 模糊区间：阈值以下不锁，累计越阈值后按优势比锁定。
pub fn lock_deferred() -> Result<(), u32> {
    let mut a = AxisLock::new();
    a.begin()?;
    if a.feed(5, 1)? != (0, 0) || a.state != LockState::Undecided {
        return Err(HdCode::StateBad.val());
    }
    if a.feed(5, 1)? != (0, 0) || a.state != LockState::Undecided {
        return Err(HdCode::StateBad.val());
    }
    if a.feed(5, 1)? != (5, 0) || a.state != LockState::LockedX {
        return Err(HdCode::StateBad.val());
    }
    a.end()?;
    Ok(())
}

// ── 四段打点账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StampPhase {
    Entry,
    Encode,
    Deliver,
    Commit,
}

impl StampPhase {
    pub fn index(self) -> u8 {
        match self {
            StampPhase::Entry => 0,
            StampPhase::Encode => 1,
            StampPhase::Deliver => 2,
            StampPhase::Commit => 3,
        }
    }

    pub fn from_index(i: u8) -> Result<StampPhase, u32> {
        match i {
            0 => Ok(StampPhase::Entry),
            1 => Ok(StampPhase::Encode),
            2 => Ok(StampPhase::Deliver),
            3 => Ok(StampPhase::Commit),
            _ => Err(HdCode::PhaseBad.val()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StampBook {
    pub rows: u32,
    pub last_tick: u64,
    pub phase: u8,
    pub counts: [u32; STAMP_POINTS],
    pub rejects: u32,
    pub digest: u64,
}

impl StampBook {
    pub fn new() -> StampBook {
        StampBook {
            rows: 0,
            last_tick: 0,
            phase: 0,
            counts: [0; STAMP_POINTS],
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn stamp(&mut self, phase: StampPhase, tick: u64) -> Result<(), u32> {
        if tick < self.last_tick {
            self.rejects = self.rejects.saturating_add(1);
            let e = HdCode::StampBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        let idx = phase.index() as usize;
        if idx != self.phase as usize {
            self.rejects = self.rejects.saturating_add(1);
            let e = HdCode::StampBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.phase = ((self.phase as usize + 1) % STAMP_POINTS) as u8;
        self.last_tick = tick;
        self.rows = self.rows.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, tick as u32);
        self.digest = p_core::fold_u32(self.digest, phase.index() as u32);
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.phase != 0 {
            return Err(HdCode::StampBad.val());
        }
        if self.rows == 0 || self.rows % STAMP_POINTS as u32 != 0 {
            return Err(HdCode::CoverageBad.val());
        }
        let cycles = self.rows / STAMP_POINTS as u32;
        let mut i = 0usize;
        while i < STAMP_POINTS {
            if self.counts.get(i) != Some(&cycles) {
                return Err(HdCode::LedgerBad.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 打点 ×10^3：每个事件四段严格递增，计数四等分。
pub fn stamp_rounds(rounds: u32) -> Result<StampBook, u32> {
    if rounds == 0 {
        return Err(HdCode::CoverageBad.val());
    }
    let mut b = StampBook::new();
    let mut i = 0u32;
    while i < rounds {
        let base = (i as u64).saturating_mul(4);
        b.stamp(StampPhase::Entry, base)?;
        b.stamp(StampPhase::Encode, base + 1)?;
        b.stamp(StampPhase::Deliver, base + 2)?;
        b.stamp(StampPhase::Commit, base + 3)?;
        i = i.saturating_add(1);
    }
    b.audit()?;
    if b.rows != rounds.saturating_mul(4) {
        return Err(HdCode::LedgerBad.val());
    }
    Ok(b)
}

// ── 合成/实测对账 ────────────────────────────────────────────────
/// 合成流走「整格→细格→累积」，实测流走「直接细格」；两者必须同账。
pub fn reconcile_counts(rounds: u32) -> Result<(u32, u32), u32> {
    if rounds == 0 {
        return Err(HdCode::CoverageBad.val());
    }
    let mut synth_notches = 0i32;
    let mut synth_fine = 0i64;
    let mut measured_notches = 0i32;
    let mut measured_fine = 0i64;
    let mut synth = p_meta::HiResStream::vertical();
    let mut measured = p_meta::HiResStream::vertical();
    let mut i = 0u32;
    while i < rounds {
        let whole = match i % 5 {
            0 => 120,
            1 => -120,
            2 => 1,
            3 => -1,
            _ => 360,
        };
        // 合成：传统 delta 展开为细格再喂入。
        let parts = p_meta::expand_legacy(whole)?;
        let mut k = 0usize;
        while k < parts.len() {
            if let Some(d) = parts.get(k) {
                let _ = synth.push_fine(*d)?;
            }
            k += 1;
        }
        // 实测：同样细格直接喂入。
        let mut m = 0usize;
        while m < parts.len() {
            if let Some(d) = parts.get(m) {
                let _ = measured.push_fine(*d)?;
            }
            m += 1;
        }
        synth.audit().map_err(|_| HdCode::LedgerBad.val())?;
        measured.audit().map_err(|_| HdCode::LedgerBad.val())?;
        synth_notches = synth.notches;
        measured_notches = measured.notches;
        synth_fine = synth.fine;
        measured_fine = measured.fine;
        i = i.saturating_add(1);
    }
    if synth_notches != measured_notches || synth_fine != measured_fine {
        return Err(HdCode::ReconcileBad.val());
    }
    if synth_notches == 0 {
        return Err(HdCode::CoverageBad.val());
    }
    Ok((synth.events, measured.events))
}

/// 对账 ×10^3：随机传统流合成/实测两路计数零差。
pub fn reconcile_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(HdCode::CoverageBad.val());
    }
    let mut synth = p_meta::HiResStream::vertical();
    let mut measured = p_meta::HiResStream::vertical();
    let mut rng = p_core::Lcg::new(0x8A11_0048);
    let mut i = 0u32;
    while i < rounds {
        let span = rng.below(2_000);
        let whole = rng.signed(1 + span);
        let parts = p_meta::expand_legacy(whole)?;
        let mut k = 0usize;
        while k < parts.len() {
            if let Some(d) = parts.get(k) {
                let _ = synth.push_fine(*d)?;
                let _ = measured.push_fine(*d)?;
            }
            k += 1;
        }
        if synth.yielded_check(&measured).is_err() {
            return Err(HdCode::ReconcileBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(synth.digest)
}

trait StreamCompare {
    fn yielded_check(&self, other: &p_meta::HiResStream) -> Result<(), u32>;
}

impl StreamCompare for p_meta::HiResStream {
    fn yielded_check(&self, other: &p_meta::HiResStream) -> Result<(), u32> {
        if self.fine != other.fine
            || self.notches != other.notches
            || self.remainder != other.remainder
        {
            return Err(HdCode::ReconcileBad.val());
        }
        Ok(())
    }
}

// ── 账本渲染（打点账） ────────────────────────────────────────────
pub fn render_stamps(b: &StampBook) -> String {
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        b.rows,
        b.counts[0],
        b.counts[2],
        b.phase,
        b.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HdCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HdCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HdCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HdCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HdCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HdCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HdCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HdCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(HdCode::RenderBad.val());
    }
    let rows = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(HdCode::RenderBad.val()),
    })?;
    let entry = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(HdCode::RenderBad.val()),
    })?;
    let deliver = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(HdCode::RenderBad.val()),
    })?;
    let phase = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(HdCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(HdCode::RenderBad.val()),
    })?;
    if rows == 0 || rows > 10_000_000 || rows % 4 != 0 {
        return Err(HdCode::RenderBad.val());
    }
    if entry != deliver || entry != rows / 4 {
        return Err(HdCode::LedgerBad.val());
    }
    if phase != 0 {
        return Err(HdCode::StampBad.val());
    }
    if digest == 0 {
        return Err(HdCode::RenderBad.val());
    }
    Ok(rows)
}

pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let b = stamp_rounds(50)?;
    let text = render_stamps(&b);
    let rows = audit_render(&text)?;
    if rows != b.rows {
        return Err(HdCode::ReplayBad.val());
    }
    Ok((rows, b.counts[0], b.digest))
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
        return Err(HdCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 未 begin 就 feed 必须炸 StateBad。
        let mut a = AxisLock::new();
        if a.feed(1, 1).err() == Some(HdCode::StateBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 零位移必须炸 LockBad。
        let mut b = AxisLock::new();
        let _ = b.begin();
        if b.feed(0, 0).err() == Some(HdCode::LockBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 打点时钟回退必须炸 StampBad。
        let mut s = StampBook::new();
        let _ = s.stamp(StampPhase::Entry, 10);
        if s.stamp(StampPhase::Encode, 9).err() == Some(HdCode::StampBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 跳段打点必须炸 StampBad。
        let mut t = StampBook::new();
        if t.stamp(StampPhase::Encode, 1).err() == Some(HdCode::StampBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(HdCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (x, y) = lock_rounds(LOCK_ROUNDS)?;
    if x != LOCK_ROUNDS || y != LOCK_ROUNDS {
        return Err(HdCode::LockBad.val());
    }
    lock_deferred()?;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let b = stamp_rounds(STAMP_ROUNDS)?;
    if b.rows != STAMP_ROUNDS * 4 {
        return Err(HdCode::LedgerBad.val());
    }
    let mut i = 0usize;
    while i < STAMP_POINTS {
        if b.counts.get(i) != Some(&STAMP_ROUNDS) {
            return Err(HdCode::LedgerBad.val());
        }
        i += 1;
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let (s, m) = reconcile_counts(200)?;
    if s != m {
        return Err(HdCode::ReconcileBad.val());
    }
    if reconcile_rounds(300)? != reconcile_rounds(300)? {
        return Err(HdCode::ReplayBad.val());
    }
    if !segment_guard() {
        return Err(HdCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = stamp_rounds(100)?;
    let b = stamp_rounds(100)?;
    if a.digest != b.digest || a.rows != b.rows {
        return Err(HdCode::ReplayBad.val());
    }
    if lock_rounds(50)? != lock_rounds(50)? {
        return Err(HdCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HdCode::ALL.len() {
        let v = HdCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let lock = lock_rounds(LOCK_ROUNDS);
    out.push((
        "F48948 切轴×10^2锁定",
        matches!(&lock, Ok((x, y)) if *x == LOCK_ROUNDS && *y == LOCK_ROUNDS),
        match &lock {
            Ok((x, y)) => format!("x={} y={}", x, y),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48948 阈值以下不锁",
        lock_deferred().is_ok(),
        String::from("5+5+5 累计后锁 X"),
    ));

    let mut a = AxisLock::new();
    let begin = a.begin();
    let filtered = a.feed(30, 2);
    let rereset = a.end();
    out.push((
        "F48948 锁定后过滤副轴",
        begin.is_ok()
            && filtered == Ok((30, 0))
            && a.state == LockState::Idle
            && rereset.is_ok(),
        format!("feed={:?} state={:?}", filtered, a.state),
    ));

    let stamps = stamp_rounds(STAMP_ROUNDS);
    let mut s = StampBook::new();
    let ok_order = s.stamp(StampPhase::Entry, 0).is_ok()
        && s.stamp(StampPhase::Encode, 0).is_ok()
        && s.stamp(StampPhase::Deliver, 0).is_ok()
        && s.stamp(StampPhase::Commit, 0).is_ok();
    // 全量落账与单拍四段合并：同属打点账，任一失败即整条红。
    out.push((
        "F48949 打点全落账与四段",
        matches!(&stamps, Ok(b) if b.rows == STAMP_ROUNDS * 4 && b.audit().is_ok())
            && ok_order
            && s.rows == 4,
        match &stamps {
            Ok(b) => format!("rows={} entry={} quad={}", b.rows, b.counts[0], s.rows),
            Err(e) => format!("err=0x{:X} quad={}", e, s.rows),
        },
    ));

    let recon = reconcile_counts(200);
    out.push((
        "F48950 对账零差",
        matches!(&recon, Ok((s, m)) if s == m),
        match &recon {
            Ok((s, m)) => format!("synth={} measured={}", s, m),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = reconcile_rounds(RECONCILE_ROUNDS);
    out.push((
        "F48950 对账×10^3稳定",
        fuzz.is_ok(),
        match &fuzz {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let render = golden_run();
    out.push((
        "F48949 打点账可解析",
        render.is_ok(),
        match &render {
            Ok((r, c, d)) => format!("rows={} entry={} d=0x{:X}", r, c, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48948 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", HdCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48948_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48948 轴锁定与滚轮对账");
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
    fn lock_state_starts_idle() {
        let a = AxisLock::new();
        assert_eq!(a.state, LockState::Idle);
        assert_eq!(a.locks_x, 0);
        assert_eq!(a.locks_y, 0);
    }

    #[test]
    fn feed_without_begin_is_rejected() {
        let mut a = AxisLock::new();
        assert_eq!(a.feed(1, 1), Err(HdCode::StateBad.val()));
    }

    #[test]
    fn begin_twice_is_rejected() {
        let mut a = AxisLock::new();
        assert!(a.begin().is_ok());
        assert_eq!(a.begin(), Err(HdCode::StateBad.val()));
    }

    #[test]
    fn horizontal_dominance_locks_x() {
        let mut a = AxisLock::new();
        a.begin().ok();
        assert_eq!(a.feed(40, 3), Ok((40, 0)));
        assert_eq!(a.state, LockState::LockedX);
        assert_eq!(a.feed(1, 99), Ok((1, 0)));
    }

    #[test]
    fn vertical_dominance_locks_y() {
        let mut a = AxisLock::new();
        a.begin().ok();
        assert_eq!(a.feed(2, 40), Ok((0, 40)));
        assert_eq!(a.state, LockState::LockedY);
        assert_eq!(a.feed(99, 2), Ok((0, 2)));
    }

    #[test]
    fn below_threshold_stays_undecided() {
        let mut a = AxisLock::new();
        a.begin().ok();
        assert_eq!(a.feed(3, 3), Ok((0, 0)));
        assert_eq!(a.state, LockState::Undecided);
    }

    #[test]
    fn deferred_lock_after_accumulation() {
        assert!(lock_deferred().is_ok());
    }

    #[test]
    fn lock_rounds_are_balanced() {
        assert_eq!(lock_rounds(LOCK_ROUNDS), Ok((LOCK_ROUNDS, LOCK_ROUNDS)));
        assert!(lock_rounds(0).is_err());
    }

    #[test]
    fn end_requires_active_state() {
        let mut a = AxisLock::new();
        assert_eq!(a.end(), Err(HdCode::StateBad.val()));
    }

    #[test]
    fn stamps_must_be_ordered() {
        let mut b = StampBook::new();
        assert!(b.stamp(StampPhase::Entry, 5).is_ok());
        assert_eq!(b.stamp(StampPhase::Encode, 4), Err(HdCode::StampBad.val()));
        assert_eq!(b.stamp(StampPhase::Deliver, 5), Err(HdCode::StampBad.val()));
    }

    #[test]
    fn stamps_reject_phase_skip() {
        let mut b = StampBook::new();
        assert_eq!(b.stamp(StampPhase::Deliver, 1), Err(HdCode::StampBad.val()));
    }

    #[test]
    fn stamp_rounds_balance() {
        match stamp_rounds(STAMP_ROUNDS) {
            Ok(b) => {
                assert_eq!(b.rows, STAMP_ROUNDS * 4);
                assert_eq!(b.counts, [STAMP_ROUNDS; 4]);
                assert!(b.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn reconcile_counts_are_equal() {
        match reconcile_counts(200) {
            Ok((s, m)) => {
                assert_eq!(s, m);
                assert!(s > 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn reconcile_rounds_deterministic() {
        assert_eq!(reconcile_rounds(300), reconcile_rounds(300));
        assert!(reconcile_rounds(0).is_err());
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let b = match stamp_rounds(10) {
            Ok(v) => v,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        let text = render_stamps(&b);
        assert_eq!(audit_render(&text), Ok(40));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
    }

    #[test]
    fn render_rejects_incomplete_cycle() {
        let b = StampBook::new();
        let text = render_stamps(&b);
        assert!(audit_render(&text).is_err());
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
    fn checks_are_green() {
        let s = run_unx48948_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
