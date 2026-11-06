//! UNX-F48941 滚轮 WHEEL_DELTA=120 语义总成 · 0x71180
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F48941-J1 delta ×10^3 换算零漂移；
//!   ③ UNX-F48942-J1 累积 ×10^3 剩余量账一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：delta 合法性 → 整格/余量拆分 → 余量并入 → 整格递送 →
//!           恒等式复算 → 账本入账。
//! 先校 delta 再拆：负 delta 与零 delta 的语义不同（零表示空事件，
//! 负表示反向），若先整除再把 0 当整格，会把空事件记成消费。
//! 恒等式放在递送之后：`总输入 = 整格×120 + 余量` 是本模块唯一可
//! 观测的守恒量；只有它成立，上层「剩余量保持」才不是空话。
//! 关键差异：本模块只回答「这段 REL_WHEEL delta 该拆出几个整格」，
//!           不做水平轴（B08-META）、不做 HI_RES 小数累积（B08-META）、
//!           不做降采样（B08-GUARD）。谁把余量丢掉，谁就违反了本模块
//!           的恒等式——余量的去向必须由调用方显式记账。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x71180，VX6 批 B08 族首槽）。
pub const SEG_BASE: u32 = 0x71180;

/// WHEEL_DELTA：Windows 沿用的滚轮整格基准。
pub const WHEEL_DELTA: i32 = 120;
/// 单次 delta 绝对值上限（Linux 输入层 int 域内自洽截断）。
pub const DELTA_MAX: i32 = 1_048_575;
/// 换算轮数（卡面判据 ×10^3 的载体）。
pub const DELTA_ROUNDS: u32 = 1_000;
/// 累积轮数（卡面判据 ×10^3 的载体）。
pub const ACC_ROUNDS: u32 = 1_000;
/// 账本字段数。
pub const LEDGER_FIELDS: usize = 4;
/// 余量绝对值上限（整除后恒 < WHEEL_DELTA）。
pub const REMAINDER_BOUND: i32 = WHEEL_DELTA - 1;

/// FNV-1a 64 初始值与质数（与 B05/B06/B07 族同参，便于跨族折叠）。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WcCode {
    /// delta 非法（为零或超界）
    DeltaBad,
    /// 整格拆分非法（整除口径被破坏）
    UnitBad,
    /// 轴非法（未知轴向）
    AxisBad,
    /// 余量非法（越出 ±119 或符号漂移）
    ResBad,
    /// 方向非法（递送方向与 delta 符号不一致）
    DirBad,
    /// 次序非法（账本计数次序断裂）
    OrderBad,
    /// 账不平（恒等式不成立）
    LedgerBad,
    /// 渲染非法（字段缺失/回读失败）
    RenderBad,
    /// 复算不一致（重放结果不同）
    ReplayBad,
    /// 覆盖不足（探针或账本项数不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x71180 窗口）
    SegmentBad,
    /// 消费非法（空事件被计入消费）
    ConsumeBad,
}

impl WcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            WcCode::DeltaBad => "delta 非法：为零或绝对值超界",
            WcCode::UnitBad => "整格拆分非法：整除口径被破坏",
            WcCode::AxisBad => "轴非法：未知轴向编号",
            WcCode::ResBad => "余量非法：越出 ±119 或符号漂移",
            WcCode::DirBad => "方向非法：递送方向与 delta 符号不一致",
            WcCode::OrderBad => "次序非法：账本计数次序断裂",
            WcCode::LedgerBad => "账不平：总输入 = 整格×120 + 余量 不成立",
            WcCode::RenderBad => "渲染非法：字段缺失或回读失败",
            WcCode::ReplayBad => "复算不一致：同一流重放结果不同",
            WcCode::CoverageBad => "覆盖不足：探针或账本项数不足",
            WcCode::SegmentBad => "段位越界：诊断码逃出 0x71180 窗口",
            WcCode::ConsumeBad => "消费非法：空事件被计入消费",
        }
    }

    pub const ALL: [WcCode; 12] = [
        WcCode::DeltaBad,
        WcCode::UnitBad,
        WcCode::AxisBad,
        WcCode::ResBad,
        WcCode::DirBad,
        WcCode::OrderBad,
        WcCode::LedgerBad,
        WcCode::RenderBad,
        WcCode::ReplayBad,
        WcCode::CoverageBad,
        WcCode::SegmentBad,
        WcCode::ConsumeBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < WcCode::ALL.len() {
        if WcCode::ALL[i].val() == code {
            return WcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 折叠与确定性伪随机 ────────────────────────────────────────────
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

pub fn fold_i32(h: u64, v: i32) -> u64 {
    fold_u32(h, v as u32)
}

/// 确定性 LCG（同族同参数，跨批不漂移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }

    /// 在 ±limit（含）内取非零有符号值。
    pub fn signed(&mut self, limit: u32) -> i32 {
        let span = limit.saturating_mul(2).saturating_add(1);
        let raw = self.below(span) as i64 - limit as i64;
        if raw == 0 {
            1
        } else {
            raw as i32
        }
    }
}

// ── 轴向 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelAxis {
    Vertical,
    Horizontal,
}

impl WheelAxis {
    pub fn index(self) -> u8 {
        match self {
            WheelAxis::Vertical => 0,
            WheelAxis::Horizontal => 1,
        }
    }

    pub fn code(self) -> u32 {
        match self {
            WheelAxis::Vertical => SEG_BASE + 0x20,
            WheelAxis::Horizontal => SEG_BASE + 0x21,
        }
    }

    pub fn from_index(i: u8) -> Result<WheelAxis, u32> {
        match i {
            0 => Ok(WheelAxis::Vertical),
            1 => Ok(WheelAxis::Horizontal),
            _ => Err(WcCode::AxisBad.val()),
        }
    }
}

// ── 整格拆分 ──────────────────────────────────────────────────────
/// 拆分口径：整除向零截断，余量与被除数同号。
///
/// `delta = notches*120 + remainder` 对正负 delta 同时成立；这正是
/// 「剩余量保持」能够跨事件累计而不偏移的代数基础。
pub fn wheel_units(delta: i32) -> Result<(i32, i32), u32> {
    if delta == 0 || (delta as i64).unsigned_abs() > DELTA_MAX as u64 {
        return Err(WcCode::DeltaBad.val());
    }
    let notches = delta / WHEEL_DELTA;
    let remainder = delta % WHEEL_DELTA;
    if remainder.unsigned_abs() > REMAINDER_BOUND as u32 {
        return Err(WcCode::ResBad.val());
    }
    Ok((notches, remainder))
}

/// 由整格与余量复原总输入（恒等式左侧）。
pub fn recompose(notches: i32, remainder: i32) -> Result<i64, u32> {
    if remainder.unsigned_abs() > REMAINDER_BOUND as u32 {
        return Err(WcCode::ResBad.val());
    }
    Ok(notches as i64 * WHEEL_DELTA as i64 + remainder as i64)
}

/// 一次 delta 恰好一个整格时的正向递送方向：-1/0/+1。
pub fn direction(delta: i32) -> Result<i8, u32> {
    if delta == 0 {
        return Err(WcCode::ConsumeBad.val());
    }
    if (delta as i64).unsigned_abs() > DELTA_MAX as u64 {
        return Err(WcCode::DeltaBad.val());
    }
    Ok(if delta > 0 {
        1
    } else {
        -1
    })
}

// ── 垂直滚轮累积器 ────────────────────────────────────────────────
/// 整数余量累积账：整格即时递送，余量留到后续事件，恒等式恒成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WheelAccumulator {
    pub axis: WheelAxis,
    pub remainder: i32,
    pub notches: i32,
    pub events: u32,
    pub rejects: u32,
    pub consults: u32,
    pub total: i64,
    pub digest: u64,
}

impl WheelAccumulator {
    pub fn new(axis: WheelAxis) -> WheelAccumulator {
        WheelAccumulator {
            axis,
            remainder: 0,
            notches: 0,
            events: 0,
            rejects: 0,
            consults: 0,
            total: 0,
            digest: FNV_OFF,
        }
    }

    pub fn vertical() -> WheelAccumulator {
        WheelAccumulator::new(WheelAxis::Vertical)
    }

    /// 吞入一段 delta，返回本次真正递送的整格数（可能为 0）。
    ///
    /// 余量先并入再整除：`新余量 = (旧余量 + delta%120) % 120` 的等价
    /// 形式由 `sum / 120` 与 `sum % 120` 一次给出，避免二次取模。
    pub fn push(&mut self, delta: i32) -> Result<i32, u32> {
        let (whole, part) = match wheel_units(delta) {
            Ok(v) => v,
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let sum = self.remainder as i64 + part as i64;
        let carry = match sum.floor_div(WHEEL_DELTA as i64) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = WcCode::UnitBad.val();
                self.digest = fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let new_rem = match sum.floor_rem(WHEEL_DELTA as i64, carry) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = WcCode::ResBad.val();
                self.digest = fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let emit64 = whole as i64 + carry;
        if emit64 < i32::MIN as i64 || emit64 > i32::MAX as i64 {
            self.rejects = self.rejects.saturating_add(1);
            let e = WcCode::OrderBad.val();
            self.digest = fold_u32(self.digest, e);
            return Err(e);
        }
        if new_rem.unsigned_abs() > REMAINDER_BOUND as u64 {
            self.rejects = self.rejects.saturating_add(1);
            let e = WcCode::ResBad.val();
            self.digest = fold_u32(self.digest, e);
            return Err(e);
        }
        let emit = emit64 as i32;
        let notches = match self.notches.checked_add(emit) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = WcCode::LedgerBad.val();
                self.digest = fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let total = match self.total.checked_add(delta as i64) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = WcCode::LedgerBad.val();
                self.digest = fold_u32(self.digest, e);
                return Err(e);
            }
        };
        self.remainder = new_rem as i32;
        self.notches = notches;
        self.total = total;
        self.events = self.events.saturating_add(1);
        self.consults = self.consults.saturating_add(1);
        self.digest = fold_i32(self.digest, delta);
        self.digest = fold_i32(self.digest, emit);
        self.digest = fold_i32(self.digest, self.remainder);
        Ok(emit)
    }

    /// 恒等式与余量域审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.remainder.unsigned_abs() > REMAINDER_BOUND as u32 {
            return Err(WcCode::ResBad.val());
        }
        let lhs = self.notches as i64 * WHEEL_DELTA as i64 + self.remainder as i64;
        if lhs != self.total {
            return Err(WcCode::LedgerBad.val());
        }
        if self.events == 0 {
            return Err(WcCode::CoverageBad.val());
        }
        if self.events.saturating_add(self.rejects) < self.consults {
            return Err(WcCode::OrderBad.val());
        }
        Ok(())
    }

    /// 方向一致性：整格符号必须与总输入符号一致（总输入为零除外）。
    pub fn direction_ok(&self) -> bool {
        if self.total == 0 {
            self.notches == 0
        } else {
            (self.notches as i64).signum() == self.total.signum() || self.notches == 0
        }
    }
}

// ── 有符号整除辅助（不依赖浮点，显式校验） ─────────────────────────
trait SignedDiv {
    fn floor_div(self, den: Self) -> Option<Self>
    where
        Self: Sized;
    fn floor_rem(self, den: Self, quot: Self) -> Option<Self>
    where
        Self: Sized;
}

impl SignedDiv for i64 {
    /// 截断整除（Rust `/` 语义）；除零返回 None。
    fn floor_div(self, den: i64) -> Option<i64> {
        if den == 0 {
            None
        } else {
            Some(self / den)
        }
    }

    /// 与截断整除配套的余数：`self == quot*den + rem`。
    fn floor_rem(self, den: i64, quot: i64) -> Option<i64> {
        if den == 0 {
            None
        } else {
            quot.checked_mul(den).and_then(|q| self.checked_sub(q))
        }
    }
}

// ── 换算与累积 ×10^3 ─────────────────────────────────────────────
fn sample_delta(i: u32) -> i32 {
    // 覆盖：整格、整格±1、余量饱和、极值边界。
    match i % 12 {
        0 => WHEEL_DELTA,
        1 => -WHEEL_DELTA,
        2 => 1,
        3 => -1,
        4 => WHEEL_DELTA - 1,
        5 => -(WHEEL_DELTA - 1),
        6 => WHEEL_DELTA + 1,
        7 => -(WHEEL_DELTA + 1),
        8 => DELTA_MAX,
        9 => -DELTA_MAX,
        10 => 7,
        _ => -7,
    }
}

/// delta ×10^3：每轮样本拆分后必须精确复原。
pub fn delta_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(WcCode::CoverageBad.val());
    }
    let mut h = FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let d = sample_delta(i);
        let (n, r) = wheel_units(d)?;
        let back = recompose(n, r)?;
        if back != d as i64 {
            return Err(WcCode::UnitBad.val());
        }
        if r.unsigned_abs() > REMAINDER_BOUND as u32 {
            return Err(WcCode::ResBad.val());
        }
        let dir = direction(d)?;
        if (d > 0) != (dir > 0) {
            return Err(WcCode::DirBad.val());
        }
        h = fold_i32(h, d);
        h = fold_i32(h, n);
        h = fold_i32(h, r);
        i = i.saturating_add(1);
    }
    Ok(h)
}

/// 累积 ×10^3：流式推送样本，逐步校验恒等式。
pub fn acc_rounds(rounds: u32) -> Result<WheelAccumulator, u32> {
    if rounds == 0 {
        return Err(WcCode::CoverageBad.val());
    }
    let mut a = WheelAccumulator::vertical();
    let mut i = 0u32;
    while i < rounds {
        let d = sample_delta(i);
        let _ = a.push(d)?;
        a.audit()?;
        if !a.direction_ok() {
            return Err(WcCode::DirBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(a)
}

/// 黄金流：+120,+1,-121,+119,-119,+240,-360,-1 每轮总输入 -121。
pub fn golden_vertical(rounds: u32) -> Result<WheelAccumulator, u32> {
    if rounds == 0 {
        return Err(WcCode::CoverageBad.val());
    }
    const STEPS: [i32; 8] = [120, 1, -121, 119, -119, 240, -360, -1];
    let mut a = WheelAccumulator::vertical();
    let mut i = 0u32;
    while i < rounds {
        let mut k = 0usize;
        while k < STEPS.len() {
            if let Some(d) = STEPS.get(k) {
                let _ = a.push(*d)?;
            }
            a.audit()?;
            k += 1;
        }
        i = i.saturating_add(1);
    }
    let want_total = -121i64 * rounds as i64;
    if a.total != want_total {
        return Err(WcCode::LedgerBad.val());
    }
    // 每轮净输入 −121 → 净整格 −1、净余量 −1：余量按轮线性累积，
    // 到 ±119 边界才由进位归一（`-N mod 120`）。首版误把余量当
    // 「每轮归零再算」，于是 100 轮后的 −100 被误判为漂移。
    let want_rem = -((rounds % WHEEL_DELTA as u32) as i32);
    if a.remainder != want_rem {
        return Err(WcCode::ResBad.val());
    }
    Ok(a)
}

/// fuzz：随机有符号 delta，恒等式必须恒成立。
pub fn fuzz_core_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(WcCode::CoverageBad.val());
    }
    let mut a = WheelAccumulator::vertical();
    let mut rng = Lcg::new(0x8A11_0041);
    let mut i = 0u32;
    while i < rounds {
        let limit = 120 + rng.below(DELTA_MAX as u32 - 120);
        let d = rng.signed(limit);
        let _ = a.push(d)?;
        if !a.direction_ok() {
            return Err(WcCode::DirBad.val());
        }
        i = i.saturating_add(1);
    }
    a.audit()?;
    Ok(a.digest)
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_ledger(a: &WheelAccumulator) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        a.events, a.notches, a.remainder, a.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(WcCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(WcCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(WcCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(WcCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_i32(s: &str) -> Result<i32, u32> {
    let bytes = s.as_bytes();
    let (neg, start) = match bytes.first() {
        Some(b'-') => (true, 1usize),
        Some(_) => (false, 0usize),
        None => return Err(WcCode::RenderBad.val()),
    };
    let digits = s.len().saturating_sub(start);
    if digits == 0 || digits > 10 {
        return Err(WcCode::RenderBad.val());
    }
    let mut v = 0i64;
    let mut i = start;
    while i < s.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(WcCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(WcCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as i64))
        {
            Some(x) => x,
            None => return Err(WcCode::RenderBad.val()),
        };
        i += 1;
    }
    if neg {
        v = -v;
    }
    if v < i32::MIN as i64 || v > i32::MAX as i64 {
        return Err(WcCode::RenderBad.val());
    }
    Ok(v as i32)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(WcCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(WcCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(WcCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(WcCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != LEDGER_FIELDS {
        return Err(WcCode::RenderBad.val());
    }
    let events = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(WcCode::RenderBad.val()),
    })?;
    let notches = parse_i32(match parts.get(1) {
        Some(v) => v,
        None => return Err(WcCode::RenderBad.val()),
    })?;
    let remainder = parse_i32(match parts.get(2) {
        Some(v) => v,
        None => return Err(WcCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(WcCode::RenderBad.val()),
    })?;
    if events == 0 || events > 10_000_000 {
        return Err(WcCode::RenderBad.val());
    }
    if remainder.unsigned_abs() > REMAINDER_BOUND as u32 {
        return Err(WcCode::ResBad.val());
    }
    if digest == 0 {
        return Err(WcCode::RenderBad.val());
    }
    let _ = notches;
    Ok(events)
}

pub fn golden_run() -> Result<(u32, i32, i32, u64), u32> {
    let a = golden_vertical(100)?;
    let text = render_ledger(&a);
    let events = audit_render(&text)?;
    if events != a.events {
        return Err(WcCode::ReplayBad.val());
    }
    Ok((events, a.notches, a.remainder, a.digest))
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
        return Err(WcCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 零 delta 必须被拒。
        if wheel_units(0).err() == Some(WcCode::DeltaBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 越界 delta 必须被拒。
        if wheel_units(DELTA_MAX + 1).err() == Some(WcCode::DeltaBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 恒等式被破坏必须炸 LedgerBad。
        let mut a = WheelAccumulator::vertical();
        let _ = a.push(WHEEL_DELTA + 5)?;
        a.total = a.total.saturating_add(1);
        if a.audit().err() == Some(WcCode::LedgerBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let mut b = WheelAccumulator::vertical();
        let _ = b.push(-WHEEL_DELTA)?;
        let text = render_ledger(&b);
        let bad = text.replacen("0x", "0y", 1);
        if audit_render(&bad).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(WcCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let h1 = delta_rounds(DELTA_ROUNDS)?;
    let h2 = delta_rounds(DELTA_ROUNDS)?;
    if h1 != h2 {
        return Err(WcCode::ReplayBad.val());
    }
    if WHEEL_DELTA != 120 || REMAINDER_BOUND != 119 {
        return Err(WcCode::UnitBad.val());
    }
    let (n, r) = wheel_units(-WHEEL_DELTA)?;
    if n != -1 || r != 0 {
        return Err(WcCode::UnitBad.val());
    }
    let (n2, r2) = wheel_units(-(WHEEL_DELTA + 1))?;
    if n2 != -1 || r2 != -1 {
        return Err(WcCode::UnitBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let mut a = WheelAccumulator::vertical();
    let e1 = a.push(-1)?;
    let e2 = a.push(-WHEEL_DELTA)?;
    let e3 = a.push(1)?;
    if e1 != 0 || e2 != -1 || e3 != 0 {
        return Err(WcCode::UnitBad.val());
    }
    if a.remainder != 0 || a.notches != -1 || a.total != -120 {
        return Err(WcCode::LedgerBad.val());
    }
    a.audit()?;
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let a = golden_vertical(50)?;
    let text = render_ledger(&a);
    if audit_render(&text)? != a.events {
        return Err(WcCode::RenderBad.val());
    }
    if !segment_guard() {
        return Err(WcCode::SegmentBad.val());
    }
    let mut i = 0usize;
    while i < WcCode::ALL.len() {
        let c = WcCode::ALL[i];
        if c.val() <= SEG_BASE || c.val() > SEG_BASE + 0x3F {
            return Err(WcCode::SegmentBad.val());
        }
        if c.detail().is_empty() || c.detail() == "未知诊断码" {
            return Err(WcCode::SegmentBad.val());
        }
        i += 1;
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = acc_rounds(200)?;
    let b = acc_rounds(200)?;
    if a.digest != b.digest || a.total != b.total || a.notches != b.notches {
        return Err(WcCode::ReplayBad.val());
    }
    if fuzz_core_rounds(300)? != fuzz_core_rounds(300)? {
        return Err(WcCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < WcCode::ALL.len() {
        let v = WcCode::ALL[i].val();
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

    out.push((
        "F48941 常量锚定",
        WHEEL_DELTA == 120 && REMAINDER_BOUND == 119 && DELTA_MAX > WHEEL_DELTA,
        format!("delta={} rem_bound={} max={}", WHEEL_DELTA, REMAINDER_BOUND, DELTA_MAX),
    ));

    let conv = delta_rounds(DELTA_ROUNDS);
    out.push((
        "F48941 换算×10^3零漂移",
        conv.is_ok(),
        match &conv {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut sym_ok = true;
    let cases = [1i32, 7, 119, 120, 121, 239, 240, 359, DELTA_MAX];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(d) = cases.get(i) {
            if let (Ok((n, r)), Ok((mn, mr))) = (wheel_units(*d), wheel_units(-*d)) {
                if n != -mn || r != -mr {
                    sym_ok = false;
                }
            } else {
                sym_ok = false;
            }
        }
        i += 1;
    }
    out.push((
        "F48941 正负拆分对称",
        sym_ok,
        String::from("逐值镜像校验"),
    ));

    let acc = acc_rounds(ACC_ROUNDS);
    out.push((
        "F48942 累积×10^3余量账",
        acc.is_ok(),
        match &acc {
            Ok(a) => format!("events={} notches={} rem={}", a.events, a.notches, a.remainder),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let golden = golden_vertical(100);
    out.push((
        "F48942 恒等式复算",
        matches!(&golden, Ok(a) if a.audit().is_ok() && a.total == -12_100 && a.remainder == -100),
        match &golden {
            Ok(a) => format!("total={} notches={} rem={}", a.total, a.notches, a.remainder),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_core_rounds(2_000);
    out.push((
        "F48942 fuzz×10^3恒等",
        fuzz.is_ok(),
        match &fuzz {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let render = golden_run();
    out.push((
        "F48941 账本可解析",
        render.is_ok(),
        match &render {
            Ok((e, n, r, d)) => format!("events={} notches={} rem={} d=0x{:X}", e, n, r, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48941 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", WcCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48941_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48941 滚轮 WHEEL_DELTA 语义总成");
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
    fn delta_rounds_are_stable() {
        assert_eq!(delta_rounds(50), delta_rounds(50));
        assert!(delta_rounds(1_000).is_ok());
        assert_eq!(delta_rounds(0), Err(WcCode::CoverageBad.val()));
    }

    #[test]
    fn zero_delta_is_rejected() {
        assert_eq!(wheel_units(0), Err(WcCode::DeltaBad.val()));
        assert_eq!(direction(0), Err(WcCode::ConsumeBad.val()));
    }

    #[test]
    fn overflow_delta_is_rejected() {
        assert_eq!(wheel_units(DELTA_MAX + 1), Err(WcCode::DeltaBad.val()));
        assert_eq!(wheel_units(-DELTA_MAX - 1), Err(WcCode::DeltaBad.val()));
    }

    #[test]
    fn split_identity_holds_for_signs() {
        let mut d = -500i32;
        while d <= 500 {
            if d != 0 {
                match wheel_units(d) {
                    Ok((n, r)) => {
                        assert_eq!(recompose(n, r), Ok(d as i64));
                        assert!(r.unsigned_abs() <= REMAINDER_BOUND as u32);
                    }
                    Err(_) => assert!(false),
                }
            }
            d += 37;
        }
    }

    #[test]
    fn remainder_is_preserved_across_events() {
        let mut a = WheelAccumulator::vertical();
        assert_eq!(a.push(100), Ok(0));
        assert_eq!(a.remainder, 100);
        assert_eq!(a.push(30), Ok(1));
        assert_eq!(a.remainder, 10);
        assert_eq!(a.push(-130), Ok(-1));
        assert_eq!(a.remainder, 0);
        assert!(a.audit().is_ok());
    }

    #[test]
    fn negative_remainder_keeps_sign() {
        let mut a = WheelAccumulator::vertical();
        assert_eq!(a.push(-100), Ok(0));
        assert_eq!(a.remainder, -100);
        assert_eq!(a.push(-30), Ok(-1));
        assert_eq!(a.remainder, -10);
        assert!(a.audit().is_ok());
    }

    #[test]
    fn golden_total_is_exact() {
        match golden_vertical(10) {
            Ok(a) => {
                assert_eq!(a.total, -1_210);
                // 10 轮 × −121 = −1210 = −10×120 + (−10)：整格恰为 −10。
                assert_eq!(a.notches, -10);
                assert_eq!(a.remainder, -10);
                assert!(a.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn acc_rounds_balance() {
        match acc_rounds(ACC_ROUNDS) {
            Ok(a) => {
                assert!(a.audit().is_ok());
                assert!(a.direction_ok());
                assert_eq!(a.events, ACC_ROUNDS);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let a = WheelAccumulator::vertical();
        let mut b = a;
        let _ = b.push(WHEEL_DELTA);
        let text = render_ledger(&b);
        assert_eq!(audit_render(&text), Ok(1));
        let bad = text.replacen("0x", "0y", 1);
        assert!(audit_render(&bad).is_err());
    }

    #[test]
    fn signed_parse_handles_minus() {
        assert_eq!(parse_i32("-1"), Ok(-1));
        assert_eq!(parse_i32("120"), Ok(120));
        assert!(parse_i32("").is_err());
        assert!(parse_i32("-").is_err());
        assert!(parse_i32("1x").is_err());
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(7) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [7, 7, 7, 7]);
            }
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
    fn fuzz_core_is_stable() {
        assert_eq!(fuzz_core_rounds(500), fuzz_core_rounds(500));
        assert!(fuzz_core_rounds(0).is_err());
    }

    #[test]
    fn direction_matches_sign() {
        assert_eq!(direction(1), Ok(1));
        assert_eq!(direction(-1), Ok(-1));
        assert_eq!(direction(WHEEL_DELTA), Ok(1));
        assert!(direction(DELTA_MAX + 1).is_err());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48941_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
