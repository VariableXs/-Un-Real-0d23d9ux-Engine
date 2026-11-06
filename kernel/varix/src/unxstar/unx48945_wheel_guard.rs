//! UNX-F48945 HI_RES→传统 WHEEL 降采样 · 0x71200
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F48945-J1 降采样 ×10^3 合成流与原始流对账一致；
//!   ③ UNX-F48946-J1 开关 ×10^2 行为一致；
//!   ④ UNX-F48947-J1 hover ×10^2 路由正确。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：细格合法性 → 合成整格 → 原始流对账 → 方向开关 →
//!           窗口路由 → 计数守恒。
//! 先对账再开关：方向反转是最后一道符号变换，若先反转再对账，
//! 合成流与原始流的差值会带上开关状态，对账就不再是纯函数。
//! 路由放在计数之前：滚轮事件只有找到接收窗口才算「已递送」，
//! 未找到时必须显式记 dropped，而不是悄悄消失。
//! 关键差异：本模块只回答「细格怎样压成传统整格、方向怎样翻、事件
//!           交给谁」，不做轴锁定（B08-DISPATCH）、不做平滑滚动实装
//!           （B08-EDGE 仅登记）、不做高频调度（B09）。hover 语义只在
//!           「窗口矩形由谁提供」上留给显示域，本模块只消费窗口号。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;
use super::unx48943_wheel_meta as p_meta;

/// 诊断段基址（0x71200，B08 族第三槽）。
pub const SEG_BASE: u32 = 0x71200;

/// 降采样轮数（卡面判据 ×10^3 的载体）。
pub const DOWNSAMPLE_ROUNDS: u32 = 1_000;
/// 方向开关轮数（卡面判据 ×10^2 的载体）。
pub const INVERT_ROUNDS: u32 = 100;
/// hover 路由轮数（卡面判据 ×10^2 的载体）。
pub const HOVER_ROUNDS: u32 = 100;
/// 同时存在的窗口号上限（路由账用）。
pub const WINDOW_MAX: u32 = 1_000_000;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HgCode {
    /// 降采样非法（合成整格越界）
    DownBad,
    /// 递送非法（发射计数越界）
    EmitBad,
    /// 反转非法（方向开关状态不一致）
    InvertBad,
    /// 路由非法（窗口号为零或越界）
    RouteBad,
    /// 目标非法（路由目标编号非法）
    TargetBad,
    /// 次序非法
    OrderBad,
    /// 账不平（合成流与原始流不一致）
    LedgerBad,
    /// 渲染非法
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 覆盖不足
    CoverageBad,
    /// 段位越界
    SegmentBad,
    /// 策略非法（开关组合不合法）
    PolicyBad,
}

impl HgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HgCode::DownBad => "降采样非法：合成整格越界或余量越界",
            HgCode::EmitBad => "递送非法：发射计数越界",
            HgCode::InvertBad => "反转非法：方向开关状态不一致",
            HgCode::RouteBad => "路由非法：窗口号为零或越界",
            HgCode::TargetBad => "目标非法：路由目标编号非法",
            HgCode::OrderBad => "次序非法：降采样账本计数次序断裂",
            HgCode::LedgerBad => "账不平：合成流与原始流对账不一致",
            HgCode::RenderBad => "渲染非法：字段缺失或回读失败",
            HgCode::ReplayBad => "复算不一致：同一流重放不同",
            HgCode::CoverageBad => "覆盖不足：探针或账本项数不足",
            HgCode::SegmentBad => "段位越界：诊断码逃出 0x71200 窗口",
            HgCode::PolicyBad => "策略非法：开关组合不合法",
        }
    }

    pub const ALL: [HgCode; 12] = [
        HgCode::DownBad,
        HgCode::EmitBad,
        HgCode::InvertBad,
        HgCode::RouteBad,
        HgCode::TargetBad,
        HgCode::OrderBad,
        HgCode::LedgerBad,
        HgCode::RenderBad,
        HgCode::ReplayBad,
        HgCode::CoverageBad,
        HgCode::SegmentBad,
        HgCode::PolicyBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HgCode::ALL.len() {
        if HgCode::ALL[i].val() == code {
            return HgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── HI_RES → 传统 WHEEL 合成器 ────────────────────────────────────
/// 老应用只认识 ±120 的传统事件；本合成器把细格流压成整格，
/// 余量留在账上等待后续细格补齐。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LegacySynth {
    pub fine: i64,
    pub emitted: i32,
    pub remainder: i32,
    pub events: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl LegacySynth {
    pub fn new() -> LegacySynth {
        LegacySynth {
            fine: 0,
            emitted: 0,
            remainder: 0,
            events: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 吞入一段细格 delta，返回本次真正发射的传统整格数。
    pub fn push_fine(&mut self, delta: i32) -> Result<i32, u32> {
        let (whole, part) = match p_meta::fine_units(delta) {
            Ok(v) => v,
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                let mapped = if e == p_meta::HmCode::DeltaBad.val() {
                    HgCode::DownBad.val()
                } else {
                    HgCode::DownBad.val()
                };
                self.digest = p_core::fold_u32(self.digest, mapped);
                return Err(mapped);
            }
        };
        let sum = self.remainder as i64 + part as i64;
        let carry = sum / 120;
        let new_rem = sum % 120;
        if new_rem.unsigned_abs() > 119 {
            self.rejects = self.rejects.saturating_add(1);
            let e = HgCode::DownBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        let emit = whole as i64 + carry;
        if emit < i32::MIN as i64 || emit > i32::MAX as i64 {
            self.rejects = self.rejects.saturating_add(1);
            let e = HgCode::EmitBad.val();
            self.digest = p_core::fold_u32(self.digest, e);
            return Err(e);
        }
        let fine = match self.fine.checked_add(delta as i64) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = HgCode::LedgerBad.val();
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        let emitted = match self.emitted.checked_add(emit as i32) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let e = HgCode::LedgerBad.val();
                self.digest = p_core::fold_u32(self.digest, e);
                return Err(e);
            }
        };
        self.fine = fine;
        self.emitted = emitted;
        self.remainder = new_rem as i32;
        self.events = self.events.saturating_add(1);
        self.digest = p_core::fold_i32(self.digest, delta);
        self.digest = p_core::fold_i32(self.digest, emit as i32);
        self.digest = p_core::fold_i32(self.digest, self.remainder);
        Ok(emit as i32)
    }

    /// 合成账审计：`细格总数 = 发射整格×120 + 余量`。
    pub fn audit(&self) -> Result<(), u32> {
        if self.remainder.unsigned_abs() > 119 {
            return Err(HgCode::DownBad.val());
        }
        let lhs = self.emitted as i64 * 120 + self.remainder as i64;
        if lhs != self.fine {
            return Err(HgCode::LedgerBad.val());
        }
        if self.events == 0 {
            return Err(HgCode::CoverageBad.val());
        }
        Ok(())
    }
}

/// 原始细格流与合成整格流对账：同一条流两路并行，恒等式必须等价。
pub fn reconcile_stream(rounds: u32) -> Result<(i64, i32, i32), u32> {
    if rounds == 0 {
        return Err(HgCode::CoverageBad.val());
    }
    let mut raw = p_meta::HiResStream::vertical();
    let mut synth = LegacySynth::new();
    let mut i = 0u32;
    while i < rounds {
        let fine = match i % 8 {
            0 => 1,
            1 => -1,
            2 => 17,
            3 => -17,
            4 => 119,
            5 => -119,
            6 => 120,
            _ => -120,
        };
        let _ = raw.push_fine(fine)?;
        let _ = synth.push_fine(fine)?;
        raw.audit().map_err(|_| HgCode::LedgerBad.val())?;
        synth.audit()?;
        if raw.fine != synth.fine || raw.remainder != synth.remainder {
            return Err(HgCode::LedgerBad.val());
        }
        if raw.notches != synth.emitted {
            return Err(HgCode::LedgerBad.val());
        }
        i = i.saturating_add(1);
    }
    // 每 8 步净额：1-1+17-17+119-119+120-120 = 0。
    if synth.fine != 0 || synth.emitted != 0 || synth.remainder != 0 {
        return Err(HgCode::LedgerBad.val());
    }
    Ok((synth.fine, synth.emitted, synth.remainder))
}

/// 降采样 ×10^3：随机细格流，两路对账逐步成立。
pub fn downsample_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(HgCode::CoverageBad.val());
    }
    let mut raw = p_meta::HiResStream::vertical();
    let mut synth = LegacySynth::new();
    let mut rng = p_core::Lcg::new(0x8A11_0045);
    let mut i = 0u32;
    while i < rounds {
        let limit = 1 + rng.below(p_meta::FINE_MAX as u32 - 1);
        let d = rng.signed(limit);
        let _ = raw.push_fine(d)?;
        let _ = synth.push_fine(d)?;
        synth.audit()?;
        if raw.fine != synth.fine || raw.notches != synth.emitted {
            return Err(HgCode::LedgerBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(synth.digest)
}

// ── 自然滚动开关 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirectionPolicy {
    pub natural: bool,
    pub flips: u32,
    pub passes: u32,
    pub digest: u64,
}

impl DirectionPolicy {
    pub fn new(natural: bool) -> DirectionPolicy {
        DirectionPolicy {
            natural,
            flips: 0,
            passes: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn apply(&mut self, delta: i32) -> Result<i32, u32> {
        if delta == 0 {
            return Err(HgCode::InvertBad.val());
        }
        if (delta as i64).unsigned_abs() > p_core::DELTA_MAX as u64 {
            return Err(HgCode::InvertBad.val());
        }
        if self.natural {
            self.flips = self.flips.saturating_add(1);
            self.digest = p_core::fold_i32(self.digest, delta);
            Ok(-delta)
        } else {
            self.passes = self.passes.saturating_add(1);
            self.digest = p_core::fold_i32(self.digest, delta);
            Ok(delta)
        }
    }

    pub fn set(&mut self, natural: bool) -> Result<(), u32> {
        if self.natural == natural {
            return Err(HgCode::PolicyBad.val());
        }
        self.natural = natural;
        Ok(())
    }

    pub fn audit(&self, rounds: u32) -> Result<(), u32> {
        if self.flips.saturating_add(self.passes) != rounds {
            return Err(HgCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 开关 ×10^2：同一输入流在两种开关态下符号必须严格镜像。
pub fn invert_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    if rounds == 0 {
        return Err(HgCode::CoverageBad.val());
    }
    let mut on = DirectionPolicy::new(true);
    let mut off = DirectionPolicy::new(false);
    let mut i = 0u32;
    while i < rounds {
        let d = if i % 2 == 0 {
            (i as i32 % 500).saturating_add(1)
        } else {
            -((i as i32 % 500).saturating_add(1))
        };
        let a = on.apply(d)?;
        let b = off.apply(d)?;
        if a != -b || b != d {
            return Err(HgCode::InvertBad.val());
        }
        i = i.saturating_add(1);
    }
    on.audit(rounds)?;
    off.audit(rounds)?;
    Ok((on.flips, off.passes))
}

// ── hover 滚动路由 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteTarget {
    /// 交给光标下的窗口（hover 滚动）
    Hover(u32),
    /// 光标下无窗口时回落到焦点窗口
    Focus(u32),
    /// 无接收者（显式记账）
    None,
}

impl RouteTarget {
    pub fn id(self) -> Option<u32> {
        match self {
            RouteTarget::Hover(id) => Some(id),
            RouteTarget::Focus(id) => Some(id),
            RouteTarget::None => None,
        }
    }

    pub fn code(self) -> u32 {
        match self {
            RouteTarget::Hover(_) => SEG_BASE + 0x30,
            RouteTarget::Focus(_) => SEG_BASE + 0x31,
            RouteTarget::None => SEG_BASE + 0x32,
        }
    }
}

/// 路由账：hover 优先、焦点回落、无接收者显式 dropped。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WheelRouter {
    pub hover: Option<u32>,
    pub focus: Option<u32>,
    pub hover_enabled: bool,
    pub routed_hover: u32,
    pub routed_focus: u32,
    pub dropped: u32,
    pub digest: u64,
}

impl WheelRouter {
    pub fn new() -> WheelRouter {
        WheelRouter {
            hover: None,
            focus: None,
            hover_enabled: true,
            routed_hover: 0,
            routed_focus: 0,
            dropped: 0,
            digest: p_core::FNV_OFF,
        }
    }

    fn check_window(id: u32) -> Result<u32, u32> {
        if id == 0 || id > WINDOW_MAX {
            return Err(HgCode::RouteBad.val());
        }
        Ok(id)
    }

    pub fn set_hover(&mut self, id: Option<u32>) -> Result<(), u32> {
        self.hover = match id {
            Some(v) => Some(WheelRouter::check_window(v)?),
            None => None,
        };
        Ok(())
    }

    pub fn set_focus(&mut self, id: Option<u32>) -> Result<(), u32> {
        self.focus = match id {
            Some(v) => Some(WheelRouter::check_window(v)?),
            None => None,
        };
        Ok(())
    }

    pub fn target(&self) -> RouteTarget {
        if self.hover_enabled {
            if let Some(id) = self.hover {
                return RouteTarget::Hover(id);
            }
        }
        if let Some(id) = self.focus {
            return RouteTarget::Focus(id);
        }
        RouteTarget::None
    }

    pub fn deliver(&mut self) -> Result<RouteTarget, u32> {
        let t = self.target();
        match t {
            RouteTarget::Hover(id) => {
                self.routed_hover = self.routed_hover.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, id);
            }
            RouteTarget::Focus(id) => {
                self.routed_focus = self.routed_focus.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, id);
            }
            RouteTarget::None => {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
        self.digest = p_core::fold_u32(self.digest, t.code());
        Ok(t)
    }

    pub fn audit(&self, rounds: u32) -> Result<(), u32> {
        if rounds == 0 {
            return Err(HgCode::CoverageBad.val());
        }
        if self.routed_hover.saturating_add(self.routed_focus).saturating_add(self.dropped)
            != rounds
        {
            return Err(HgCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// hover ×10^2：hover 优先、focus 回落、双空丢弃的份额必须精确。
pub fn hover_rounds(rounds: u32) -> Result<(u32, u32, u32), u32> {
    if rounds == 0 {
        return Err(HgCode::CoverageBad.val());
    }
    let mut r = WheelRouter::new();
    let mut hover_n = 0u32;
    let mut focus_n = 0u32;
    let mut none_n = 0u32;
    let mut i = 0u32;
    while i < rounds {
        match i % 4 {
            0 => {
                r.set_hover(Some(10 + i % 7))?;
                r.set_focus(Some(99))?;
                r.hover_enabled = true;
                if let RouteTarget::Hover(id) = r.deliver()? {
                    if id != 10 + i % 7 {
                        return Err(HgCode::TargetBad.val());
                    }
                    hover_n = hover_n.saturating_add(1);
                } else {
                    return Err(HgCode::TargetBad.val());
                }
            }
            1 => {
                r.set_hover(None)?;
                r.set_focus(Some(99))?;
                if let RouteTarget::Focus(id) = r.deliver()? {
                    if id != 99 {
                        return Err(HgCode::TargetBad.val());
                    }
                    focus_n = focus_n.saturating_add(1);
                } else {
                    return Err(HgCode::TargetBad.val());
                }
            }
            2 => {
                r.set_hover(Some(7))?;
                r.hover_enabled = false;
                r.set_focus(Some(99))?;
                if let RouteTarget::Focus(id) = r.deliver()? {
                    if id != 99 {
                        return Err(HgCode::TargetBad.val());
                    }
                    focus_n = focus_n.saturating_add(1);
                } else {
                    return Err(HgCode::TargetBad.val());
                }
                r.hover_enabled = true;
            }
            _ => {
                r.set_hover(None)?;
                r.set_focus(None)?;
                if r.deliver()? != RouteTarget::None {
                    return Err(HgCode::TargetBad.val());
                }
                none_n = none_n.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    r.audit(rounds)?;
    if r.routed_hover != hover_n || r.routed_focus != focus_n || r.dropped != none_n {
        return Err(HgCode::LedgerBad.val());
    }
    Ok((hover_n, focus_n, none_n))
}

/// fuzz：乱窗口组合，计数守恒必须恒成立。
pub fn fuzz_router_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(HgCode::CoverageBad.val());
    }
    let mut r = WheelRouter::new();
    let mut rng = p_core::Lcg::new(0x8A11_0047);
    let mut i = 0u32;
    while i < rounds {
        let hv = match rng.below(3) {
            0 => None,
            _ => Some(1 + rng.below(WINDOW_MAX)),
        };
        let fc = match rng.below(3) {
            0 => None,
            _ => Some(1 + rng.below(WINDOW_MAX)),
        };
        r.set_hover(hv)?;
        r.set_focus(fc)?;
        r.hover_enabled = rng.below(2) == 0;
        let _ = r.deliver()?;
        i = i.saturating_add(1);
    }
    r.audit(rounds)?;
    Ok(r.digest)
}

// ── 账本渲染（合成器） ────────────────────────────────────────────
pub fn render_synth(s: &LegacySynth) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        s.events, s.emitted, s.remainder, s.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HgCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HgCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HgCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HgCode::RenderBad.val()),
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
        None => return Err(HgCode::RenderBad.val()),
    };
    if s.len().saturating_sub(start) == 0 {
        return Err(HgCode::RenderBad.val());
    }
    let mut v = 0i64;
    let mut i = start;
    while i < s.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HgCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HgCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as i64))
        {
            Some(x) => x,
            None => return Err(HgCode::RenderBad.val()),
        };
        i += 1;
    }
    if neg {
        v = -v;
    }
    if v < i32::MIN as i64 || v > i32::MAX as i64 {
        return Err(HgCode::RenderBad.val());
    }
    Ok(v as i32)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HgCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HgCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HgCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HgCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(HgCode::RenderBad.val());
    }
    let events = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(HgCode::RenderBad.val()),
    })?;
    let emitted = parse_i32(match parts.get(1) {
        Some(v) => v,
        None => return Err(HgCode::RenderBad.val()),
    })?;
    let remainder = parse_i32(match parts.get(2) {
        Some(v) => v,
        None => return Err(HgCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(HgCode::RenderBad.val()),
    })?;
    if events == 0 || events > 10_000_000 {
        return Err(HgCode::RenderBad.val());
    }
    if remainder.unsigned_abs() > 119 {
        return Err(HgCode::DownBad.val());
    }
    if digest == 0 {
        return Err(HgCode::RenderBad.val());
    }
    let _ = emitted;
    Ok(events)
}

pub fn golden_run() -> Result<(u32, i32, i32, u64), u32> {
    let mut s = LegacySynth::new();
    let mut i = 0u32;
    while i < 100 {
        let d = match i % 6 {
            0 => 17,
            1 => -17,
            2 => 119,
            3 => -119,
            4 => 120,
            _ => -120,
        };
        let _ = s.push_fine(d)?;
        i = i.saturating_add(1);
    }
    s.audit()?;
    let text = render_synth(&s);
    if audit_render(&text)? != s.events {
        return Err(HgCode::ReplayBad.val());
    }
    Ok((s.events, s.emitted, s.remainder, s.digest))
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
        return Err(HgCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 零细格必须被拒。
        let mut s = LegacySynth::new();
        if s.push_fine(0).err() == Some(HgCode::DownBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 合成账被破坏必须炸 LedgerBad。
        let mut t = LegacySynth::new();
        let _ = t.push_fine(17)?;
        t.fine = t.fine.saturating_add(1);
        if t.audit().err() == Some(HgCode::LedgerBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 窗口号零必须被拒。
        let mut r = WheelRouter::new();
        if r.set_hover(Some(0)).err() == Some(HgCode::RouteBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let mut u = LegacySynth::new();
        let _ = u.push_fine(120)?;
        let text = render_synth(&u).replacen("0x", "0y", 1);
        if audit_render(&text).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(HgCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let mut s = LegacySynth::new();
    if s.push_fine(119)? != 0 {
        return Err(HgCode::DownBad.val());
    }
    if s.push_fine(1)? != 1 {
        return Err(HgCode::DownBad.val());
    }
    if s.remainder != 0 || s.emitted != 1 || s.fine != 120 {
        return Err(HgCode::LedgerBad.val());
    }
    s.audit()?;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let (h, f, n) = hover_rounds(HOVER_ROUNDS)?;
    if h.saturating_add(f).saturating_add(n) != HOVER_ROUNDS {
        return Err(HgCode::LedgerBad.val());
    }
    if h == 0 || f == 0 || n == 0 {
        return Err(HgCode::CoverageBad.val());
    }
    let (flips, passes) = invert_rounds(INVERT_ROUNDS)?;
    if flips != INVERT_ROUNDS || passes != INVERT_ROUNDS {
        return Err(HgCode::InvertBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let (f, e, r) = reconcile_stream(200)?;
    if f != 0 || e != 0 || r != 0 {
        return Err(HgCode::LedgerBad.val());
    }
    if downsample_rounds(300)? != downsample_rounds(300)? {
        return Err(HgCode::ReplayBad.val());
    }
    if !segment_guard() {
        return Err(HgCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = hover_rounds(80)?;
    let b = hover_rounds(80)?;
    if a != b {
        return Err(HgCode::ReplayBad.val());
    }
    if fuzz_router_rounds(300)? != fuzz_router_rounds(300)? {
        return Err(HgCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HgCode::ALL.len() {
        let v = HgCode::ALL[i].val();
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

    let recon = reconcile_stream(200);
    out.push((
        "F48945 合成流对账中性",
        recon == Ok((0, 0, 0)),
        match &recon {
            Ok((f, e, r)) => format!("fine={} emitted={} rem={}", f, e, r),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let down = downsample_rounds(DOWNSAMPLE_ROUNDS);
    out.push((
        "F48945 降采样×10^3零差",
        down.is_ok(),
        match &down {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut split_ok = true;
    let mut s = LegacySynth::new();
    if s.push_fine(60) != Ok(0) || s.push_fine(60) != Ok(1) {
        split_ok = false;
    }
    if s.remainder != 0 || s.emitted != 1 {
        split_ok = false;
    }
    out.push((
        "F48945 细格两段补整",
        split_ok,
        format!("emitted={} rem={}", s.emitted, s.remainder),
    ));

    let inv = invert_rounds(INVERT_ROUNDS);
    let mut pol = DirectionPolicy::new(false);
    let same = pol.set(false);
    // 镜像计数与空切换合并为一条：两者同属方向策略，任一失败即整条红。
    out.push((
        "F48946 反转镜像与空切换",
        inv == Ok((INVERT_ROUNDS, INVERT_ROUNDS))
            && same == Err(HgCode::PolicyBad.val()),
        match &inv {
            Ok((f, p)) => format!("flips={} passes={} set={:?}", f, p, same),
            Err(e) => format!("err=0x{:X} set={:?}", e, same),
        },
    ));

    let hover = hover_rounds(HOVER_ROUNDS);
    out.push((
        "F48947 hover×10^2份额",
        matches!(&hover, Ok((h, f, n)) if h + f + n == HOVER_ROUNDS),
        match &hover {
            Ok((h, f, n)) => format!("hover={} focus={} none={}", h, f, n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut r = WheelRouter::new();
    r.set_hover(Some(5)).ok();
    r.set_focus(Some(9)).ok();
    let t1 = r.deliver();
    r.set_hover(None).ok();
    let t2 = r.deliver();
    r.set_focus(None).ok();
    let t3 = r.deliver();
    out.push((
        "F48947 三级回落次序",
        t1 == Ok(RouteTarget::Hover(5))
            && t2 == Ok(RouteTarget::Focus(9))
            && t3 == Ok(RouteTarget::None),
        format!("t1={:?} t2={:?} t3={:?}", t1, t2, t3),
    ));

    let render = golden_run();
    out.push((
        "F48945 账本可解析",
        render.is_ok(),
        match &render {
            Ok((e, m, r, d)) => format!("events={} emitted={} rem={} d=0x{:X}", e, m, r, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48945 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", HgCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48945_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48945 HI_RES 降采样与 hover 路由");
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
    fn synth_accumulates_fractions() {
        let mut s = LegacySynth::new();
        assert_eq!(s.push_fine(119), Ok(0));
        assert_eq!(s.push_fine(1), Ok(1));
        assert_eq!(s.fine, 120);
        assert_eq!(s.emitted, 1);
        assert_eq!(s.remainder, 0);
        assert!(s.audit().is_ok());
    }

    #[test]
    fn synth_rejects_zero() {
        let mut s = LegacySynth::new();
        assert_eq!(s.push_fine(0), Err(HgCode::DownBad.val()));
        assert_eq!(s.rejects, 1);
    }

    #[test]
    fn synth_negative_fractions() {
        let mut s = LegacySynth::new();
        assert_eq!(s.push_fine(-119), Ok(0));
        assert_eq!(s.push_fine(-1), Ok(-1));
        assert_eq!(s.fine, -120);
        assert_eq!(s.emitted, -1);
    }

    #[test]
    fn reconcile_is_neutral() {
        assert_eq!(reconcile_stream(200), Ok((0, 0, 0)));
        assert_eq!(reconcile_stream(0), Err(HgCode::CoverageBad.val()));
    }

    #[test]
    fn downsample_is_deterministic() {
        assert_eq!(downsample_rounds(300), downsample_rounds(300));
        assert!(downsample_rounds(0).is_err());
    }

    #[test]
    fn natural_scroll_flips() {
        let mut on = DirectionPolicy::new(true);
        let mut off = DirectionPolicy::new(false);
        assert_eq!(on.apply(120), Ok(-120));
        assert_eq!(off.apply(120), Ok(120));
        assert_eq!(on.flips, 1);
        assert_eq!(off.passes, 1);
    }

    #[test]
    fn invert_rejects_zero_and_overflow() {
        let mut p = DirectionPolicy::new(true);
        assert_eq!(p.apply(0), Err(HgCode::InvertBad.val()));
        assert_eq!(p.apply(p_core::DELTA_MAX + 1), Err(HgCode::InvertBad.val()));
    }

    #[test]
    fn policy_set_requires_change() {
        let mut p = DirectionPolicy::new(false);
        assert_eq!(p.set(false), Err(HgCode::PolicyBad.val()));
        assert!(p.set(true).is_ok());
    }

    #[test]
    fn invert_rounds_are_mirrored() {
        assert_eq!(invert_rounds(INVERT_ROUNDS), Ok((INVERT_ROUNDS, INVERT_ROUNDS)));
        assert!(invert_rounds(0).is_err());
    }

    #[test]
    fn router_prefers_hover() {
        let mut r = WheelRouter::new();
        assert!(r.set_hover(Some(5)).is_ok());
        assert!(r.set_focus(Some(9)).is_ok());
        assert_eq!(r.deliver(), Ok(RouteTarget::Hover(5)));
        assert_eq!(r.routed_hover, 1);
    }

    #[test]
    fn router_falls_back_to_focus() {
        let mut r = WheelRouter::new();
        assert!(r.set_focus(Some(9)).is_ok());
        assert_eq!(r.deliver(), Ok(RouteTarget::Focus(9)));
        assert_eq!(r.routed_focus, 1);
    }

    #[test]
    fn router_drops_without_target() {
        let mut r = WheelRouter::new();
        assert_eq!(r.deliver(), Ok(RouteTarget::None));
        assert_eq!(r.dropped, 1);
    }

    #[test]
    fn router_disabled_hover_uses_focus() {
        let mut r = WheelRouter::new();
        assert!(r.set_hover(Some(5)).is_ok());
        assert!(r.set_focus(Some(9)).is_ok());
        r.hover_enabled = false;
        assert_eq!(r.deliver(), Ok(RouteTarget::Focus(9)));
    }

    #[test]
    fn router_rejects_bad_window() {
        let mut r = WheelRouter::new();
        assert_eq!(r.set_hover(Some(0)), Err(HgCode::RouteBad.val()));
        assert_eq!(r.set_focus(Some(WINDOW_MAX + 1)), Err(HgCode::RouteBad.val()));
    }

    #[test]
    fn hover_rounds_conserve() {
        match hover_rounds(HOVER_ROUNDS) {
            Ok((h, f, n)) => {
                assert_eq!(h + f + n, HOVER_ROUNDS);
                assert!(h > 0 && f > 0 && n > 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let mut s = LegacySynth::new();
        let _ = s.push_fine(120);
        let text = render_synth(&s);
        assert_eq!(audit_render(&text), Ok(1));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
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
        let s = run_unx48945_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
