//! UNX-F48943 水平滚轮与 HI_RES 高解析流 · 0x711C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F48943-J1 横向 ×10^3 一致；
//!   ③ UNX-F48944-J1 HI_RES ×10^3 流解析零丢。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：轴向合法性 → 细粒度 delta 合法性 → 细格累积 →
//!           整格换算 → 余量保持 → 轴向隔离复算。
//! 先校轴再校值：水平与垂直共用同一套 120 基准，但余量账必须分轴。
//! 若先接受数值再发现轴向非法，这段 delta 会被记到错误轴上，后续
//! 「横向一致」就无从谈起。
//! 余量保持放在换算之后：HI_RES 的细格流允许 1/120 级别的输入，若每次
//! 都重新取整，低速细流会整段消失；只有余量跨事件存活，流解析才零丢。
//! 关键差异：本模块只回答「水平轴与高解析流怎样折成整格」，不做
//!           整格到应用的降采样（B08-GUARD）、不做方向反转（B08-GUARD）、
//!           不做轴锁定（B08-DISPATCH）。谁把细格直接截断成 0，谁就
//!           丢失了高解析设备存在的意义。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x711C0，B08 族第二槽）。
pub const SEG_BASE: u32 = 0x711C0;

/// HI_RES 细格基准：一个传统整格 = 120 细格。
pub const FINE_DELTA: i32 = p_core::WHEEL_DELTA;
/// 单次细格绝对值上限。
pub const FINE_MAX: i32 = p_core::DELTA_MAX;
/// 横向轮数（卡面判据 ×10^3 的载体）。
pub const HORIZONTAL_ROUNDS: u32 = 1_000;
/// HI_RES 轮数（卡面判据 ×10^3 的载体）。
pub const HIRES_ROUNDS: u32 = 1_000;
/// 账本字段数。
pub const LEDGER_FIELDS: usize = 5;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HmCode {
    /// 轴向非法
    AxisBad,
    /// 细格 delta 非法（为零或超界）
    DeltaBad,
    /// 余量非法（越出 ±119）
    ResBad,
    /// HI_RES 输入非法（非细格口径）
    HiResBad,
    /// 分数累积非法（分数与整格对不上）
    FractionBad,
    /// 次序非法
    OrderBad,
    /// 账不平（细格恒等式不成立）
    LedgerBad,
    /// 渲染非法
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 覆盖不足
    CoverageBad,
    /// 段位越界
    SegmentBad,
    /// 流非法（事件流形状不合法）
    StreamBad,
}

impl HmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HmCode::AxisBad => "轴向非法：未知轴向编号",
            HmCode::DeltaBad => "细格 delta 非法：为零或绝对值超界",
            HmCode::ResBad => "余量非法：越出 ±119",
            HmCode::HiResBad => "HI_RES 输入非法：不是细格口径",
            HmCode::FractionBad => "分数累积非法：分数与整格对不上",
            HmCode::OrderBad => "次序非法：细格账本计数次序断裂",
            HmCode::LedgerBad => "账不平：细格总数 = 整格×120 + 余量 不成立",
            HmCode::RenderBad => "渲染非法：字段缺失或回读失败",
            HmCode::ReplayBad => "复算不一致：同一细格流重放不同",
            HmCode::CoverageBad => "覆盖不足：探针或账本项数不足",
            HmCode::SegmentBad => "段位越界：诊断码逃出 0x711C0 窗口",
            HmCode::StreamBad => "流非法：事件流形状不合法",
        }
    }

    pub const ALL: [HmCode; 12] = [
        HmCode::AxisBad,
        HmCode::DeltaBad,
        HmCode::ResBad,
        HmCode::HiResBad,
        HmCode::FractionBad,
        HmCode::OrderBad,
        HmCode::LedgerBad,
        HmCode::RenderBad,
        HmCode::ReplayBad,
        HmCode::CoverageBad,
        HmCode::SegmentBad,
        HmCode::StreamBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HmCode::ALL.len() {
        if HmCode::ALL[i].val() == code {
            return HmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 轴向与细格换算 ────────────────────────────────────────────────
pub use p_core::WheelAxis;

/// 细格拆分：`fine = notches*120 + remainder`（同位整数代数）。
pub fn fine_units(fine: i32) -> Result<(i32, i32), u32> {
    if fine == 0 {
        return Err(HmCode::DeltaBad.val());
    }
    if (fine as i64).unsigned_abs() > FINE_MAX as u64 {
        return Err(HmCode::DeltaBad.val());
    }
    let n = fine / FINE_DELTA;
    let r = fine % FINE_DELTA;
    if r.unsigned_abs() > (FINE_DELTA - 1) as u32 {
        return Err(HmCode::ResBad.val());
    }
    Ok((n, r))
}

pub fn recompose_fine(notches: i32, remainder: i32) -> Result<i64, u32> {
    if remainder.unsigned_abs() > (FINE_DELTA - 1) as u32 {
        return Err(HmCode::ResBad.val());
    }
    Ok(notches as i64 * FINE_DELTA as i64 + remainder as i64)
}

/// 传统 WHEEL delta → 细格流（一个整格展开成 120 细格）。
pub fn expand_legacy(delta: i32) -> Result<Vec<i32>, u32> {
    let (n, r) = p_core::wheel_units(delta)?;
    let mut out = Vec::new();
    let mut i = 0i32;
    let steps = n.unsigned_abs();
    let sign = if n >= 0 { 1 } else { -1 };
    while i < steps as i32 {
        out.push(sign * FINE_DELTA);
        i += 1;
    }
    if r != 0 {
        out.push(r);
    }
    if out.is_empty() {
        return Err(HmCode::StreamBad.val());
    }
    Ok(out)
}

// ── HI_RES 细格流累积器 ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HiResStream {
    pub axis: WheelAxis,
    pub fine: i64,
    pub notches: i32,
    pub remainder: i32,
    pub events: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl HiResStream {
    pub fn new(axis: WheelAxis) -> HiResStream {
        HiResStream {
            axis,
            fine: 0,
            notches: 0,
            remainder: 0,
            events: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn vertical() -> HiResStream {
        HiResStream::new(WheelAxis::Vertical)
    }

    pub fn horizontal() -> HiResStream {
        HiResStream::new(WheelAxis::Horizontal)
    }

    /// 吞入一段细格 delta，返回本次递送的整格数。
    pub fn push_fine(&mut self, delta: i32) -> Result<i32, u32> {
        let (whole, part) = match fine_units(delta) {
            Ok(v) => v,
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let sum = self.remainder as i64 + part as i64;
        let carry = sum / FINE_DELTA as i64;
        let new_rem = sum % FINE_DELTA as i64;
        if new_rem.unsigned_abs() > (FINE_DELTA - 1) as u64 {
            self.rejects = self.rejects.saturating_add(1);
            let e = HmCode::ResBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        let emit64 = whole as i64 + carry;
        if emit64 < i32::MIN as i64 || emit64 > i32::MAX as i64 {
            self.rejects = self.rejects.saturating_add(1);
            let e = HmCode::OrderBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        let fine = match self.fine.checked_add(delta as i64) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = HmCode::LedgerBad.val();
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let notches = match self.notches.checked_add(emit64 as i32) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = HmCode::LedgerBad.val();
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        self.fine = fine;
        self.notches = notches;
        self.remainder = new_rem as i32;
        self.events = self.events.saturating_add(1);
        self.digest = p_core::fold_i32(self.digest, delta);
        self.digest = p_core::fold_i32(self.digest, emit64 as i32);
        self.digest = p_core::fold_i32(self.digest, self.remainder);
        Ok(emit64 as i32)
    }

    /// 恒等式：细格总数 = 整格×120 + 余量；余量必须在 ±119。
    pub fn audit(&self) -> Result<(), u32> {
        if self.remainder.unsigned_abs() > (FINE_DELTA - 1) as u32 {
            return Err(HmCode::ResBad.val());
        }
        let lhs = self.notches as i64 * FINE_DELTA as i64 + self.remainder as i64;
        if lhs != self.fine {
            return Err(HmCode::LedgerBad.val());
        }
        if self.events == 0 {
            return Err(HmCode::CoverageBad.val());
        }
        Ok(())
    }

    /// 细格流折算到传统整格（向下取整到 120 的整数倍，余量不动）。
    pub fn legacy_notches(&self) -> i32 {
        self.notches
    }

    /// 轴向必须与流对象一致：用于跨轴错投的显式防御。
    pub fn axis_matches(&self, ev_axis: WheelAxis) -> bool {
        self.axis == ev_axis
    }
}

// ── 横向 ×10^3 与 HI_RES ×10^3 ───────────────────────────────────
fn horizontal_sample(i: u32) -> i32 {
    match i % 8 {
        0 => 120,
        1 => -120,
        2 => 1,
        3 => -1,
        4 => 119,
        5 => -119,
        6 => 121,
        _ => -121,
    }
}

/// 横向 ×10^3：垂直/水平各自记账，互不串轴。
pub fn horizontal_rounds(rounds: u32) -> Result<(i64, i64), u32> {
    if rounds == 0 {
        return Err(HmCode::CoverageBad.val());
    }
    let mut v = p_core::WheelAccumulator::vertical();
    let mut h = p_core::WheelAccumulator::new(WheelAxis::Horizontal);
    let mut i = 0u32;
    while i < rounds {
        let d = horizontal_sample(i);
        let _ = v.push(d)?;
        let _ = h.push(d)?;
        v.audit().map_err(|_| HmCode::LedgerBad.val())?;
        h.audit().map_err(|_| HmCode::LedgerBad.val())?;
        if v.axis != WheelAxis::Vertical || h.axis != WheelAxis::Horizontal {
            return Err(HmCode::AxisBad.val());
        }
        i = i.saturating_add(1);
    }
    if v.notches != h.notches || v.remainder != h.remainder || v.total != h.total {
        return Err(HmCode::FractionBad.val());
    }
    Ok((v.total, h.total))
}

/// HI_RES ×10^3：细格流逐步审计，最终整格与余量必须可复算。
pub fn hires_rounds(rounds: u32) -> Result<HiResStream, u32> {
    if rounds == 0 {
        return Err(HmCode::CoverageBad.val());
    }
    let mut s = HiResStream::vertical();
    let mut i = 0u32;
    while i < rounds {
        let fine = match i % 10 {
            0 => 1,
            1 => -1,
            2 => 17,
            3 => -17,
            4 => 119,
            5 => -119,
            6 => 120,
            7 => -120,
            8 => 121,
            _ => -121,
        };
        let _ = s.push_fine(fine)?;
        s.audit()?;
        i = i.saturating_add(1);
    }
    // 每 10 步净额：1-1+17-17+119-119+120-120+121-121 = 0。
    if s.fine != 0 || s.notches != 0 || s.remainder != 0 {
        return Err(HmCode::FractionBad.val());
    }
    Ok(s)
}

/// 细格流与整格流对账：120 个 +1 细格必须恰好产生一个整格。
pub fn fine_to_whole(units: u32) -> Result<i32, u32> {
    if units == 0 || units > 10_000 {
        return Err(HmCode::CoverageBad.val());
    }
    let mut s = HiResStream::vertical();
    let mut i = 0u32;
    while i < units {
        let _ = s.push_fine(1)?;
        s.audit()?;
        i = i.saturating_add(1);
    }
    s.audit()?;
    let want_notches = (units / FINE_DELTA as u32) as i32;
    let want_rem = (units % FINE_DELTA as u32) as i32;
    if s.notches != want_notches || s.remainder != want_rem {
        return Err(HmCode::FractionBad.val());
    }
    Ok(s.notches)
}

/// 黄金 HI_RES 流：交替 ±1 与 ±119，返回确定性账本。
pub fn golden_hires(rounds: u32) -> Result<HiResStream, u32> {
    if rounds == 0 {
        return Err(HmCode::CoverageBad.val());
    }
    let mut s = HiResStream::vertical();
    const STEPS: [i32; 6] = [17, -17, 1, -1, 119, -119];
    let mut i = 0u32;
    while i < rounds {
        let mut k = 0usize;
        while k < STEPS.len() {
            if let Some(d) = STEPS.get(k) {
                let _ = s.push_fine(*d)?;
            }
            s.audit()?;
            k += 1;
        }
        i = i.saturating_add(1);
    }
    if s.fine != 0 || s.remainder != 0 || s.notches != 0 {
        return Err(HmCode::FractionBad.val());
    }
    Ok(s)
}

/// fuzz：随机细格流，逐轮恒等式。
pub fn fuzz_hires_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(HmCode::CoverageBad.val());
    }
    let mut s = HiResStream::vertical();
    let mut rng = p_core::Lcg::new(0x8A11_0043);
    let mut i = 0u32;
    while i < rounds {
        let limit = 1 + rng.below(FINE_MAX as u32 - 1);
        let d = rng.signed(limit);
        let _ = s.push_fine(d)?;
        s.audit()?;
        i = i.saturating_add(1);
    }
    Ok(s.digest)
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_stream(s: &HiResStream) -> String {
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        s.events,
        s.axis.index(),
        s.notches,
        s.remainder,
        s.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HmCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HmCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HmCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HmCode::RenderBad.val()),
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
        None => return Err(HmCode::RenderBad.val()),
    };
    if s.len().saturating_sub(start) == 0 {
        return Err(HmCode::RenderBad.val());
    }
    let mut v = 0i64;
    let mut i = start;
    while i < s.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HmCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HmCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as i64))
        {
            Some(x) => x,
            None => return Err(HmCode::RenderBad.val()),
        };
        i += 1;
    }
    if neg {
        v = -v;
    }
    if v < i32::MIN as i64 || v > i32::MAX as i64 {
        return Err(HmCode::RenderBad.val());
    }
    Ok(v as i32)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HmCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HmCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HmCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HmCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<(u32, u8), u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != LEDGER_FIELDS {
        return Err(HmCode::RenderBad.val());
    }
    let events = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(HmCode::RenderBad.val()),
    })?;
    let axis_raw = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(HmCode::RenderBad.val()),
    })?;
    if axis_raw > 1 {
        return Err(HmCode::AxisBad.val());
    }
    let notches = parse_i32(match parts.get(2) {
        Some(v) => v,
        None => return Err(HmCode::RenderBad.val()),
    })?;
    let remainder = parse_i32(match parts.get(3) {
        Some(v) => v,
        None => return Err(HmCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(HmCode::RenderBad.val()),
    })?;
    if events == 0 || events > 10_000_000 {
        return Err(HmCode::RenderBad.val());
    }
    if remainder.unsigned_abs() > (FINE_DELTA - 1) as u32 {
        return Err(HmCode::ResBad.val());
    }
    if digest == 0 {
        return Err(HmCode::RenderBad.val());
    }
    let _ = notches;
    Ok((events, axis_raw as u8))
}

pub fn golden_run() -> Result<(u32, i32, i32, u64), u32> {
    let s = golden_hires(100)?;
    let text = render_stream(&s);
    let (events, axis) = audit_render(&text)?;
    if events != s.events || axis != s.axis.index() {
        return Err(HmCode::ReplayBad.val());
    }
    Ok((events, s.notches, s.remainder, s.digest))
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
        return Err(HmCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 零细格必须被拒。
        if fine_units(0).err() == Some(HmCode::DeltaBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 细格越界必须被拒。
        if fine_units(FINE_MAX + 1).err() == Some(HmCode::DeltaBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 恒等式被破坏必须炸 LedgerBad。
        let mut s = HiResStream::vertical();
        let _ = s.push_fine(17)?;
        s.fine = s.fine.saturating_add(1);
        if s.audit().err() == Some(HmCode::LedgerBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 轴号越界渲染必须被拒。
        let mut t = HiResStream::horizontal();
        let _ = t.push_fine(120)?;
        let text = render_stream(&t).replacen("|1|", "|9|", 1);
        if audit_render(&text).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(HmCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if fine_units(120)? != (1, 0) {
        return Err(HmCode::HiResBad.val());
    }
    if fine_units(119)? != (0, 119) {
        return Err(HmCode::HiResBad.val());
    }
    if fine_units(-121)? != (-1, -1) {
        return Err(HmCode::HiResBad.val());
    }
    let mut s = HiResStream::vertical();
    let a = s.push_fine(119)?;
    let b = s.push_fine(1)?;
    if a != 0 || b != 1 || s.remainder != 0 || s.notches != 1 {
        return Err(HmCode::FractionBad.val());
    }
    s.audit()?;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let (v, h) = horizontal_rounds(HORIZONTAL_ROUNDS)?;
    if v != h {
        return Err(HmCode::AxisBad.val());
    }
    let mut s = HiResStream::horizontal();
    let _ = s.push_fine(60)?;
    let _ = s.push_fine(-60)?;
    if s.remainder != 0 || s.notches != 0 {
        return Err(HmCode::FractionBad.val());
    }
    s.audit()?;
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let s = golden_hires(50)?;
    let text = render_stream(&s);
    if audit_render(&text)? != (s.events, s.axis.index()) {
        return Err(HmCode::RenderBad.val());
    }
    if !segment_guard() {
        return Err(HmCode::SegmentBad.val());
    }
    if fine_to_whole(240)? != 2 {
        return Err(HmCode::FractionBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = hires_rounds(200)?;
    let b = hires_rounds(200)?;
    if a.digest != b.digest || a.fine != b.fine || a.notches != b.notches {
        return Err(HmCode::ReplayBad.val());
    }
    if fuzz_hires_rounds(300)? != fuzz_hires_rounds(300)? {
        return Err(HmCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HmCode::ALL.len() {
        let v = HmCode::ALL[i].val();
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

    let hz = horizontal_rounds(HORIZONTAL_ROUNDS);
    out.push((
        "F48943 横向×10^3一致",
        matches!(&hz, Ok((v, h)) if v == h),
        match &hz {
            Ok((v, h)) => format!("vertical={} horizontal={}", v, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let stream = hires_rounds(HIRES_ROUNDS);
    out.push((
        "F48944 HI_RES×10^3零丢",
        stream.is_ok(),
        match &stream {
            Ok(s) => format!("events={} fine={} notches={}", s.events, s.fine, s.notches),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let whole = fine_to_whole(240);
    out.push((
        "F48944 细格折整格",
        whole == Ok(2),
        match &whole {
            Ok(n) => format!("240 fine -> {} notches", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut keep_ok = true;
    let mut s = HiResStream::vertical();
    if s.push_fine(119) != Ok(0) || s.push_fine(1) != Ok(1) {
        keep_ok = false;
    }
    if s.remainder != 0 {
        keep_ok = false;
    }
    out.push((
        "F48944 余量跨事件存活",
        keep_ok,
        format!("rem={} notches={}", s.remainder, s.notches),
    ));

    let mut neg_ok = true;
    let mut n = HiResStream::vertical();
    if n.push_fine(-119) != Ok(0) || n.push_fine(-1) != Ok(-1) {
        neg_ok = false;
    }
    if n.remainder != 0 {
        neg_ok = false;
    }
    out.push((
        "F48943 负向细格对称",
        neg_ok,
        format!("rem={} notches={}", n.remainder, n.notches),
    ));

    let expand = expand_legacy(-(p_core::WHEEL_DELTA + 1));
    out.push((
        "F48943 整格展开细格",
        matches!(&expand, Ok(v) if v.len() == 2 && v.first() == Some(&-120) && v.get(1) == Some(&-1)),
        match &expand {
            Ok(v) => format!("len={} head={:?}", v.len(), v.first()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_hires_rounds(2_000);
    out.push((
        "F48944 fuzz×10^3恒等",
        fuzz.is_ok(),
        match &fuzz {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48943 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", HmCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48943_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48943 水平滚轮与 HI_RES 高解析流");
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
    fn fine_units_split_exactly() {
        assert_eq!(fine_units(120), Ok((1, 0)));
        assert_eq!(fine_units(-120), Ok((-1, 0)));
        assert_eq!(fine_units(121), Ok((1, 1)));
        assert_eq!(fine_units(-121), Ok((-1, -1)));
        assert_eq!(fine_units(119), Ok((0, 119)));
    }

    #[test]
    fn zero_fine_is_rejected() {
        assert_eq!(fine_units(0), Err(HmCode::DeltaBad.val()));
    }

    #[test]
    fn fine_overflow_is_rejected() {
        assert_eq!(fine_units(FINE_MAX + 1), Err(HmCode::DeltaBad.val()));
        assert_eq!(fine_units(-FINE_MAX - 1), Err(HmCode::DeltaBad.val()));
    }

    #[test]
    fn fractional_stream_accumulates() {
        let mut s = HiResStream::vertical();
        assert_eq!(s.push_fine(60), Ok(0));
        assert_eq!(s.push_fine(60), Ok(1));
        assert_eq!(s.remainder, 0);
        assert_eq!(s.fine, 120);
        assert!(s.audit().is_ok());
    }

    #[test]
    fn negative_fraction_is_symmetric() {
        let mut s = HiResStream::vertical();
        assert_eq!(s.push_fine(-60), Ok(0));
        assert_eq!(s.push_fine(-60), Ok(-1));
        assert_eq!(s.remainder, 0);
        assert_eq!(s.fine, -120);
    }

    #[test]
    fn hires_rounds_net_zero() {
        match hires_rounds(HIRES_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.fine, 0);
                assert_eq!(s.notches, 0);
                assert_eq!(s.remainder, 0);
                assert!(s.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn horizontal_axes_are_isolated() {
        match horizontal_rounds(100) {
            Ok((v, h)) => {
                // 8 步一循环净零，100 步 = 12 循环 + 4 步（净零）：两轴
                // 必须逐字段一致且总账归零。
                assert_eq!(v, h);
                assert_eq!(v, 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fine_to_whole_is_exact() {
        assert_eq!(fine_to_whole(120), Ok(1));
        assert_eq!(fine_to_whole(240), Ok(2));
        assert_eq!(fine_to_whole(360), Ok(3));
        assert_eq!(fine_to_whole(0), Err(HmCode::CoverageBad.val()));
    }

    #[test]
    fn golden_hires_is_neutral() {
        match golden_hires(20) {
            Ok(s) => {
                assert_eq!(s.fine, 0);
                assert_eq!(s.notches, 0);
                assert_eq!(s.remainder, 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let mut s = HiResStream::horizontal();
        let _ = s.push_fine(120);
        let text = render_stream(&s);
        assert_eq!(audit_render(&text), Ok((1, 1)));
        let bad = text.replacen("0x", "0y", 1);
        assert!(audit_render(&bad).is_err());
    }

    #[test]
    fn render_rejects_bad_axis() {
        let mut s = HiResStream::vertical();
        let _ = s.push_fine(120);
        let text = render_stream(&s).replacen("|0|", "|2|", 1);
        assert_eq!(audit_render(&text).err(), Some(HmCode::AxisBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn expand_legacy_shape() {
        assert_eq!(expand_legacy(120), Ok(alloc::vec![120]));
        assert_eq!(expand_legacy(-120), Ok(alloc::vec![-120]));
        assert_eq!(expand_legacy(121), Ok(alloc::vec![120, 1]));
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
        let s = run_unx48943_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
