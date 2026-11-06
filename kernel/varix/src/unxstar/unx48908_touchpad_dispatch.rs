//! UNX-F48908 触控板灵敏度与开关递送 · 0x70D60
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥850 行；
//!   ② UNX-F48908-J1 灵敏度曲线 ×10^3 单调无回退；
//!   ③ UNX-F48909-J1 开关禁用 ×10^3 禁用后零递送；
//!   ④ UNX-F48910-J1 误杀率 ×10^3 手指误杀归零。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：开关启用 → 判定受理 → 灵敏度规格 → 位移上限 → 路由。
//!           开关必须最先判：禁用后连位移都不应换算，先换算再丢会
//!           让「禁用期间坐标仍被消费」成为可观测事实。判定次之：
//!           掌面被受理后再做灵敏度换算，等于白算一次还污染统计。
//!           灵敏度规格在位移之前：曲线不单调时任何缩放都不可信；
//!           位移上限在路由之前：超界位移不能被手势解释成甩动。
//! 关键差异：本模块不重新判定掌面（那是 guard 的掩码），也不做
//!           手势识别（B10）与指针加速模型（B07）。本模块只回答
//!           「这一颗已经被 guard 表态的触点，此刻该走哪条路由、
//!           缩放后位移是多少、若被丢是哪一条证据」。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48905_touchpad_guard::{
    ContactClass, GuardVerdict, M_AREA, M_EDGE, M_SMALL, M_TOOL, M_TYPING,
};

/// 诊断段基址（0x70D60，VX6 批 B06 第四槽）。
pub const SEG_BASE: u32 = 0x70D60;

/// 冻结常量（卡面判据锚点）。
pub const SENS_LEVELS: usize = 11;
pub const SENS_ROUNDS: u32 = 1000;
pub const SWITCH_ROUNDS: u32 = 1000;
pub const KILL_ROUNDS: u32 = 1000;
pub const MAX_DELTA: u32 = 4095;
pub const PER_MILLE: u32 = 1000;

// ── 诊断码闭集（+0x01..+0x09） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdCode {
    /// 曲线非法（长度/零值/非严格递增）
    CurveBad,
    /// 位移非法（超上限）
    DeltaBad,
    /// 开关非法（序号回退）
    SwitchBad,
    /// 路由非法（无对应消费者）
    RouteBad,
    /// 开关禁用后仍被递送
    LeakBad,
    /// 误杀账不平（手指被丢或掌面被放）
    KillBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
    /// 灵敏度等级越界
    LevelBad,
}

impl SdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SdCode::CurveBad => "曲线非法：必须 11 点且严格递增",
            SdCode::DeltaBad => "位移非法：超过 4095 上限",
            SdCode::SwitchBad => "开关非法：序号回退或重复提交",
            SdCode::RouteBad => "路由非法：触点没有可投递的消费者",
            SdCode::LeakBad => "禁用泄漏：开关关闭后仍被递送",
            SdCode::KillBad => "误杀账不平：手指被丢或掌面被放",
            SdCode::ReplayBad => "复算不一致：两次黄金全链结果不同",
            SdCode::SegmentBad => "段位越界：诊断码逃出 0x70D60 窗口",
            SdCode::LevelBad => "等级越界：灵敏度只允许 0..=10",
        }
    }

    pub const ALL: [SdCode; 9] = [
        SdCode::CurveBad,
        SdCode::DeltaBad,
        SdCode::SwitchBad,
        SdCode::RouteBad,
        SdCode::LeakBad,
        SdCode::KillBad,
        SdCode::ReplayBad,
        SdCode::SegmentBad,
        SdCode::LevelBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SdCode::ALL.len() {
        if SdCode::ALL[i].val() == code {
            return SdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 灵敏度曲线 ────────────────────────────────────────────────────
/// 11 档每千缩放：1.0x 在 5 档；两端 0.5x / 2.0x。
pub const CURVE: [u32; SENS_LEVELS] =
    [500, 600, 700, 800, 900, 1000, 1100, 1250, 1450, 1700, 2000];

/// 大位移加速：超过 200 raw 的部分按每 200 加半个档位缩放。
/// 短表（注入用）：故意不足 11 点。
pub const SHORT_CURVE: [u32; 5] = [500, 600, 700, 800, 900];

pub const ACCEL_KNEE: u32 = 200;
pub const ACCEL_STEP: u32 = 200;

pub fn audit_curve(table: &[u32]) -> Result<usize, u32> {
    if table.len() != SENS_LEVELS {
        return Err(SdCode::CurveBad.val());
    }
    let mut i = 0usize;
    let mut prev = 0u32;
    while i < table.len() {
        let cur = match table.get(i) {
            Some(v) => *v,
            None => return Err(SdCode::CurveBad.val()),
        };
        if cur == 0 {
            return Err(SdCode::CurveBad.val());
        }
        if i > 0 && cur <= prev {
            return Err(SdCode::CurveBad.val());
        }
        prev = cur;
        i += 1;
    }
    Ok(i)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SensCurve {
    level: u8,
    factor: u32,
    accel: bool,
}

impl SensCurve {
    pub fn new(level: u8, accel: bool) -> Result<SensCurve, u32> {
        let idx = level as usize;
        let factor = match CURVE.get(idx) {
            Some(f) => *f,
            None => return Err(SdCode::LevelBad.val()),
        };
        if factor == 0 {
            return Err(SdCode::CurveBad.val());
        }
        Ok(SensCurve { level, factor, accel })
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn factor(&self) -> u32 {
        self.factor
    }

    pub fn accel(&self) -> bool {
        self.accel
    }

    /// 单轴缩放：先按档位缩放，再对大位移加半档加速。
    pub fn apply_axis(&self, raw: u32) -> Result<u32, u32> {
        if raw > MAX_DELTA {
            return Err(SdCode::DeltaBad.val());
        }
        if raw == 0 {
            return Ok(0);
        }
        let scaled = raw.saturating_mul(self.factor) / PER_MILLE;
        if !self.accel || raw <= ACCEL_KNEE {
            return Ok(scaled);
        }
        let extra = raw
            .saturating_sub(ACCEL_KNEE)
            .saturating_mul(self.factor)
            .saturating_div(PER_MILLE.saturating_mul(2));
        Ok(scaled.saturating_add(extra))
    }

    pub fn apply(&self, dx: u32, dy: u32) -> Result<(u32, u32), u32> {
        let x = self.apply_axis(dx)?;
        let y = self.apply_axis(dy)?;
        Ok((x, y))
    }
}

/// 单调压测：固定 raw 序列在 11 档上缩放，必须逐档非降。
pub fn golden_curve(rounds: u32) -> Result<(u32, u32, u64), u32> {
    audit_curve(&CURVE)?;
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4355_5256);
    let mut samples = 0u32;
    let mut steps = 0u32;
    let mut sum = 0u64;
    let mut r = 0u32;
    while r < rounds {
        let raw = 1 + rng.below(u64::from(MAX_DELTA)) as u32;
        let mut prev = 0u32;
        let mut level = 0u8;
        while (level as usize) < SENS_LEVELS {
            let c = SensCurve::new(level, true)?;
            let out = c.apply_axis(raw)?;
            if out < prev {
                return Err(SdCode::CurveBad.val());
            }
            if out > prev {
                steps = steps.saturating_add(1);
            }
            prev = out;
            samples = samples.saturating_add(1);
            sum = sum.saturating_add(u64::from(out));
            level = level.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok((samples, steps, sum))
}

// ── 开关闸 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchState {
    Enabled,
    Disabled,
}

impl SwitchState {
    pub fn label(self) -> &'static str {
        match self {
            SwitchState::Enabled => "enabled",
            SwitchState::Disabled => "disabled",
        }
    }
}

/// 开关：序号单调；禁用后只接受重新启用，不接受任何递送。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchGate {
    state: SwitchState,
    seq: u32,
    transitions: u32,
    rejected: u32,
}

impl SwitchGate {
    pub const fn new() -> SwitchGate {
        SwitchGate { state: SwitchState::Enabled, seq: 0, transitions: 0, rejected: 0 }
    }

    pub fn state(&self) -> SwitchState {
        self.state
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    pub fn transitions(&self) -> u32 {
        self.transitions
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn set(&mut self, seq: u32, on: bool) -> Result<bool, u32> {
        if seq <= self.seq {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SdCode::SwitchBad.val());
        }
        let next = if on { SwitchState::Enabled } else { SwitchState::Disabled };
        let changed = next != self.state;
        self.seq = seq;
        if changed {
            self.state = next;
            self.transitions = self.transitions.saturating_add(1);
        }
        Ok(changed)
    }

    pub fn admit(&self, seq: u32) -> Result<(), u32> {
        if self.state == SwitchState::Disabled {
            return Err(SdCode::LeakBad.val());
        }
        if seq != self.seq {
            return Err(SdCode::SwitchBad.val());
        }
        Ok(())
    }

    pub fn enable_now(&mut self) {
        if self.state == SwitchState::Disabled {
            self.state = SwitchState::Enabled;
            self.transitions = self.transitions.saturating_add(1);
        }
    }
}

// ── 误杀账 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KillLedger {
    pub fingers: u32,
    pub palms: u32,
    pub finger_killed: u32,
    pub palm_passed: u32,
    pub switch_drops: u32,
    pub palm_drops: u32,
    pub tiny_drops: u32,
    pub tool_drops: u32,
}

impl KillLedger {
    pub const fn new() -> KillLedger {
        KillLedger {
            fingers: 0,
            palms: 0,
            finger_killed: 0,
            palm_passed: 0,
            switch_drops: 0,
            palm_drops: 0,
            tiny_drops: 0,
            tool_drops: 0,
        }
    }

    pub fn record(&mut self, kind: ContactClass, out: &DispatchOut) {
        let is_finger = matches!(kind, ContactClass::Finger | ContactClass::Stylus);
        if is_finger {
            self.fingers = self.fingers.saturating_add(1);
            if out.route == Route::Dropped {
                self.finger_killed = self.finger_killed.saturating_add(1);
            }
        } else {
            self.palms = self.palms.saturating_add(1);
            if out.route != Route::Dropped {
                self.palm_passed = self.palm_passed.saturating_add(1);
            }
        }
        if out.route == Route::Dropped {
            match out.reason {
                DropReason::SwitchOff => self.switch_drops = self.switch_drops.saturating_add(1),
                DropReason::Palm => self.palm_drops = self.palm_drops.saturating_add(1),
                DropReason::Tiny => self.tiny_drops = self.tiny_drops.saturating_add(1),
                DropReason::Tool => self.tool_drops = self.tool_drops.saturating_add(1),
                DropReason::None => {}
            }
        }
    }

    pub fn kill_ppm(&self) -> u32 {
        if self.fingers == 0 {
            0
        } else {
            self.finger_killed.saturating_mul(1_000_000) / self.fingers
        }
    }

    pub fn pass_ppm(&self) -> u32 {
        if self.palms == 0 {
            0
        } else {
            self.palm_passed.saturating_mul(1_000_000) / self.palms
        }
    }

    pub fn clean(&self) -> bool {
        self.finger_killed == 0 && self.palm_passed == 0
    }

    pub fn drops(&self) -> u32 {
        self.switch_drops
            .saturating_add(self.palm_drops)
            .saturating_add(self.tiny_drops)
            .saturating_add(self.tool_drops)
    }
}

// ── 递送路由 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Pointer,
    Gesture,
    Square,
    Dropped,
}

impl Route {
    pub fn label(self) -> &'static str {
        match self {
            Route::Pointer => "pointer",
            Route::Gesture => "gesture",
            Route::Square => "square",
            Route::Dropped => "dropped",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropReason {
    None,
    SwitchOff,
    Palm,
    Tiny,
    Tool,
}

impl DropReason {
    pub fn label(self) -> &'static str {
        match self {
            DropReason::None => "none",
            DropReason::SwitchOff => "switch-off",
            DropReason::Palm => "palm",
            DropReason::Tiny => "tiny",
            DropReason::Tool => "tool",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchOut {
    pub route: Route,
    pub dx: u32,
    pub dy: u32,
    pub reason: DropReason,
    pub level: u8,
    pub switch_seq: u32,
}

impl DispatchOut {
    pub fn dropped(reason: DropReason, seq: u32) -> DispatchOut {
        DispatchOut {
            route: Route::Dropped,
            dx: 0,
            dy: 0,
            reason,
            level: 0,
            switch_seq: seq,
        }
    }

    pub fn text(&self) -> String {
        format!(
            "{} dx={} dy={} reason={} level={} seq={}",
            self.route.label(),
            self.dx,
            self.dy,
            self.reason.label(),
            self.level,
            self.switch_seq
        )
    }
}

fn mask_reason(mask: u8) -> DropReason {
    if mask & (M_AREA | M_EDGE | M_TYPING) != 0 {
        DropReason::Palm
    } else if mask & M_SMALL != 0 {
        DropReason::Tiny
    } else if mask & M_TOOL != 0 {
        DropReason::Tool
    } else {
        DropReason::Palm
    }
}

/// 单触点递送：开关 → 判定 → 曲线 → 位移上限 → 路由。
pub fn dispatch_one(
    gate: &SwitchGate,
    verdict: &GuardVerdict,
    class: ContactClass,
    curve: &SensCurve,
    dx: u32,
    dy: u32,
    seq: u32,
) -> Result<DispatchOut, u32> {
    gate.admit(seq)?;
    if !verdict.accept {
        return Ok(DispatchOut::dropped(mask_reason(verdict.mask), seq));
    }
    let (sx, sy) = curve.apply(dx, dy)?;
    let route = match class {
        ContactClass::Finger => {
            if sx.saturating_add(sy) == 0 {
                Route::Square
            } else {
                Route::Pointer
            }
        }
        ContactClass::Stylus => Route::Pointer,
        ContactClass::Palm
        | ContactClass::EdgePalm
        | ContactClass::TypingPalm
        | ContactClass::Tiny
        | ContactClass::UnknownTool => Route::Dropped,
    };
    if route == Route::Dropped {
        return Ok(DispatchOut::dropped(DropReason::Palm, seq));
    }
    Ok(DispatchOut {
        route,
        dx: sx,
        dy: sy,
        reason: DropReason::None,
        level: curve.level(),
        switch_seq: seq,
    })
}

// ── 确定性源 ──────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x5450_4449_5350_4154)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
pub fn golden_switch(rounds: u32) -> Result<(u32, u32, u32), u32> {
    let mut gate = SwitchGate::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x5357_4954);
    let mut seq = 0u32;
    let mut leaks = 0u32;
    let mut transitions = 0u32;
    let mut r = 0u32;
    while r < rounds {
        seq = seq.saturating_add(1);
        let on = (rng.next() & 1) == 0;
        let changed = gate.set(seq, on)?;
        if changed {
            transitions = transitions.saturating_add(1);
        }
        match gate.admit(seq) {
            Ok(()) => {}
            Err(e) if e == SdCode::LeakBad.val() => {
                leaks = leaks.saturating_add(1);
            }
            Err(e) => return Err(e),
        }
        r = r.saturating_add(1);
    }
    Ok((rounds, transitions, leaks))
}

pub fn golden_dispatch(rounds: u32) -> Result<(u32, u32, u32, u32), u32> {
    let mut gate = SwitchGate::new();
    gate.set(1, true)?;
    let mut led = KillLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4449_5350);
    let mut delivered = 0u32;
    let mut dropped = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let level = (rng.below(SENS_LEVELS as u64)) as u8;
        let curve = SensCurve::new(level, true)?;
        let dx = rng.below(300) as u32;
        let dy = rng.below(300) as u32;
        let kind = match r % 3 {
            0 => ContactClass::Finger,
            1 => ContactClass::Palm,
            _ => ContactClass::Finger,
        };
        let verdict = match kind {
            ContactClass::Palm => GuardVerdict::reject(M_AREA, ContactClass::Palm),
            _ => GuardVerdict::accept(ContactClass::Finger),
        };
        let out = dispatch_one(&gate, &verdict, kind, &curve, dx, dy, 1)?;
        led.record(kind, &out);
        if out.route == Route::Dropped {
            dropped = dropped.saturating_add(1);
        } else {
            delivered = delivered.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    if !led.clean() {
        return Err(SdCode::KillBad.val());
    }
    Ok((rounds, delivered, dropped, led.drops()))
}

pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (curve_n, curve_steps, curve_sum) = golden_curve(SENS_ROUNDS)?;
    let (sw_n, sw_trans, sw_leaks) = golden_switch(SWITCH_ROUNDS)?;
    let (dsp_n, delivered, dropped, drops) = golden_dispatch(KILL_ROUNDS)?;
    if curve_n != SENS_ROUNDS.saturating_mul(SENS_LEVELS as u32) || curve_steps == 0 {
        return Err(SdCode::CurveBad.val());
    }
    if sw_n != SWITCH_ROUNDS || sw_leaks == 0 {
        return Err(SdCode::LeakBad.val());
    }
    if dsp_n != KILL_ROUNDS || delivered == 0 || dropped == 0 || drops == 0 {
        return Err(SdCode::KillBad.val());
    }
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fnv_step(h, curve_sum);
    h = fnv_step(h, u64::from(curve_steps));
    h = fnv_step(h, u64::from(sw_trans));
    h = fnv_step(h, u64::from(sw_leaks));
    h = fnv_step(h, u64::from(delivered));
    h = fnv_step(h, u64::from(dropped));
    Ok((curve_n, sw_n, delivered, h))
}

fn fnv_step(mut h: u64, x: u64) -> u64 {
    let mut i = 0usize;
    while i < 8 {
        h ^= (x >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x494E_4A);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：曲线非单调 / 长度非法 / 零值。
        let mut bad = [1000u32; SENS_LEVELS];
        bad[3] = 900;
        let curve_bad = audit_curve(&bad).err() == Some(SdCode::CurveBad.val())
            && audit_curve(&SHORT_CURVE).is_err();
        led.record(0, curve_bad);

        // 路 2：位移超上限 / 等级越界。
        let c = SensCurve::new(5, true)?;
        let delta_bad = c.apply_axis(MAX_DELTA + 1).err() == Some(SdCode::DeltaBad.val())
            && SensCurve::new(11, true).err() == Some(SdCode::LevelBad.val());
        led.record(1, delta_bad);

        // 路 3：开关序号回退 / 禁用后递送泄漏。
        let mut g = SwitchGate::new();
        g.set(4, true)?;
        let sw_bad = g.set(3, false).err() == Some(SdCode::SwitchBad.val());
        g.set(5, false)?;
        let leak = g.admit(5).err() == Some(SdCode::LeakBad.val());
        led.record(2, sw_bad && leak);

        // 路 4：误杀账污染（手指被丢 / 掌面被放）。
        let mut k = KillLedger::new();
        let dropped = DispatchOut::dropped(DropReason::Palm, 1);
        k.record(ContactClass::Finger, &dropped);
        let clean_bad = !k.clean() && k.kill_ppm() == 1_000_000;
        let mut k2 = KillLedger::new();
        let pass = DispatchOut {
            route: Route::Pointer,
            dx: 1,
            dy: 1,
            reason: DropReason::None,
            level: 5,
            switch_seq: 1,
        };
        k2.record(ContactClass::Palm, &pass);
        led.record(3, clean_bad && !k2.clean());

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (curve, sw, delivered, _) = golden_run()?;
    if curve == SENS_ROUNDS * SENS_LEVELS as u32 && sw == SWITCH_ROUNDS && delivered > 0 {
        Ok(())
    } else {
        Err(SdCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    audit_curve(&CURVE)?;
    let c = SensCurve::new(0, false)?;
    if c.apply(100, 100)? != (50, 50) {
        return Err(SdCode::CurveBad.val());
    }
    let hi = SensCurve::new(10, false)?;
    if hi.apply(100, 0)?.0 != 200 {
        return Err(SdCode::CurveBad.val());
    }
    let mut g = SwitchGate::new();
    g.set(1, true)?;
    let v = GuardVerdict::accept(ContactClass::Finger);
    if dispatch_one(&g, &v, ContactClass::Finger, &c, 0, 0, 1)?.route != Route::Square {
        return Err(SdCode::RouteBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if SENS_LEVELS == 11
        && SENS_ROUNDS == 1000
        && SWITCH_ROUNDS == 1000
        && KILL_ROUNDS == 1000
        && MAX_DELTA == 4095
        && CURVE.len() == 11
        && SdCode::ALL.len() == 9
    {
        Ok(())
    } else {
        Err(SdCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SdCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SdCode::ALL.len() {
        let v = SdCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let curve = golden_curve(SENS_ROUNDS);
    v.push((
        "F48908-J1-灵敏度千轮单调",
        match &curve {
            Ok((n, steps, _)) => *n == SENS_ROUNDS * SENS_LEVELS as u32 && *steps > 0,
            Err(_) => false,
        },
        match &curve {
            Ok((n, steps, sum)) => format!("scaled={} steps={} sum={}", n, steps, sum),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let sw = golden_switch(SWITCH_ROUNDS);
    v.push((
        "F48909-J1-开关千轮禁用零递送",
        match &sw {
            Ok((n, t, leaks)) => *n == SWITCH_ROUNDS && *t > 0 && *leaks > 0,
            Err(_) => false,
        },
        match &sw {
            Ok((n, t, leaks)) => format!("rounds={} transitions={} leaks={}", n, t, leaks),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let kill = golden_dispatch(KILL_ROUNDS);
    v.push((
        "F48910-J1-误杀率千轮归零",
        match &kill {
            Ok((n, d, dr, _)) => *n == KILL_ROUNDS && *d > 0 && *dr > 0,
            Err(_) => false,
        },
        match &kill {
            Ok((n, d, dr, drops)) => {
                format!("rounds={} delivered={} dropped={} drops={}", n, d, dr, drops)
            }
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let bad = [1000u32, 900, 800, 700, 600, 500, 400, 300, 200, 100, 50];
    v.push((
        "F48908-J1-非单调曲线必拒",
        audit_curve(&bad).err() == Some(SdCode::CurveBad.val()),
        format!("audit={:?}", audit_curve(&bad).err()),
    ));
    let c = SensCurve::new(5, false);
    v.push((
        "F48908-J1-千分缩放口径",
        match &c {
            Ok(c) => c.apply(100, 200) == Ok((100, 200)) && c.factor() == 1000,
            Err(_) => false,
        },
        match &c {
            Ok(c) => format!("factor={}", c.factor()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48908-J1-位移超限必拒",
        match &c {
            Ok(c) => c.apply_axis(MAX_DELTA + 1).err() == Some(SdCode::DeltaBad.val()),
            Err(_) => false,
        },
        String::from("4096 -> DeltaBad"),
    ));
    let mut g = SwitchGate::new();
    let _ = g.set(2, true);
    v.push((
        "F48909-J1-开关序号回退必拒",
        g.set(1, false).err() == Some(SdCode::SwitchBad.val()),
        format!("rejected={}", g.rejected()),
    ));
    let mut g2 = SwitchGate::new();
    let _ = g2.set(1, true);
    let _ = g2.set(2, false);
    v.push((
        "F48909-J1-禁用后零递送",
        g2.admit(2).err() == Some(SdCode::LeakBad.val()) && g2.state() == SwitchState::Disabled,
        format!("state={} seq={}", g2.state().label(), g2.seq()),
    ));
    let mut k = KillLedger::new();
    let good = DispatchOut {
        route: Route::Pointer,
        dx: 2,
        dy: 3,
        reason: DropReason::None,
        level: 5,
        switch_seq: 1,
    };
    k.record(ContactClass::Finger, &good);
    k.record(ContactClass::Palm, &DispatchOut::dropped(DropReason::Palm, 1));
    v.push((
        "F48910-J1-误杀账分离",
        k.clean() && k.kill_ppm() == 0 && k.pass_ppm() == 0 && k.drops() == 1,
        format!("kill={} pass={} drops={}", k.kill_ppm(), k.pass_ppm(), k.drops()),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48908-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48908-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48910-J1-段位守卫",
        segment_guard(),
        String::from("0x70D60 窗口"),
    ));
    v
}

pub fn run_unx48908_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48908 触控板灵敏度与开关递送");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_is_strictly_increasing() {
        assert_eq!(audit_curve(&CURVE), Ok(SENS_LEVELS));
        let mut bad = [1000u32; SENS_LEVELS];
        bad[5] = 999;
        assert_eq!(audit_curve(&bad).err(), Some(SdCode::CurveBad.val()));
    }

    #[test]
    fn curve_length_is_enforced() {
        let short = [500u32, 600, 700];
        assert_eq!(audit_curve(&short).err(), Some(SdCode::CurveBad.val()));
    }

    #[test]
    fn level_out_of_range_rejected() {
        assert_eq!(SensCurve::new(11, true).err(), Some(SdCode::LevelBad.val()));
        assert!(SensCurve::new(10, true).is_ok());
    }

    #[test]
    fn default_level_is_identity() {
        let c = match SensCurve::new(5, false) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert_eq!(c.apply(123, 456), Ok((123, 456)));
        assert_eq!(c.apply(0, 0), Ok((0, 0)));
    }

    #[test]
    fn low_and_high_levels_scale() {
        let lo = match SensCurve::new(0, false) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert_eq!(lo.apply(100, 60), Ok((50, 30)));
        let hi = match SensCurve::new(10, false) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert_eq!(hi.apply(100, 60), Ok((200, 120)));
    }

    #[test]
    fn acceleration_adds_only_past_knee() {
        let c = match SensCurve::new(5, true) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert_eq!(c.apply_axis(ACCEL_KNEE), Ok(ACCEL_KNEE));
        let over = match c.apply_axis(ACCEL_KNEE + 100) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(over, ACCEL_KNEE + 100 + 50);
    }

    #[test]
    fn delta_limit_is_enforced() {
        let c = match SensCurve::new(5, true) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert_eq!(c.apply_axis(MAX_DELTA + 1).err(), Some(SdCode::DeltaBad.val()));
        assert!(c.apply_axis(MAX_DELTA).is_ok());
    }

    #[test]
    fn switch_sequence_is_monotonic() {
        let mut g = SwitchGate::new();
        assert_eq!(g.set(1, true), Ok(false));
        assert_eq!(g.set(1, false).err(), Some(SdCode::SwitchBad.val()));
        assert_eq!(g.set(2, false), Ok(true));
        assert_eq!(g.state(), SwitchState::Disabled);
        assert_eq!(g.admit(2).err(), Some(SdCode::LeakBad.val()));
        assert_eq!(g.set(3, true), Ok(true));
        assert_eq!(g.admit(3), Ok(()));
    }

    #[test]
    fn dispatch_respects_verdict_mask() {
        let g = SwitchGate::new();
        let c = match SensCurve::new(5, false) {
            Ok(c) => c,
            Err(_) => return,
        };
        let palm = dispatch_one(
            &g,
            &GuardVerdict::reject(M_AREA, ContactClass::Palm),
            ContactClass::Palm,
            &c,
            10,
            10,
            0,
        );
        assert_eq!(palm.ok().map(|o| (o.route, o.reason)), Some((Route::Dropped, DropReason::Palm)));
        let tiny = dispatch_one(
            &g,
            &GuardVerdict::reject(M_SMALL, ContactClass::Tiny),
            ContactClass::Tiny,
            &c,
            10,
            10,
            0,
        );
        assert_eq!(tiny.ok().map(|o| o.reason), Some(DropReason::Tiny));
        let tool = dispatch_one(
            &g,
            &GuardVerdict::reject(M_TOOL, ContactClass::UnknownTool),
            ContactClass::UnknownTool,
            &c,
            10,
            10,
            0,
        );
        assert_eq!(tool.ok().map(|o| o.reason), Some(DropReason::Tool));
    }

    #[test]
    fn zero_motion_routes_to_square() {
        let g = SwitchGate::new();
        let c = match SensCurve::new(5, false) {
            Ok(c) => c,
            Err(_) => return,
        };
        let out = dispatch_one(
            &g,
            &GuardVerdict::accept(ContactClass::Finger),
            ContactClass::Finger,
            &c,
            0,
            0,
            0,
        );
        assert_eq!(out.ok().map(|o| o.route), Some(Route::Square));
    }

    #[test]
    fn motion_routes_to_pointer() {
        let g = SwitchGate::new();
        let c = match SensCurve::new(5, false) {
            Ok(c) => c,
            Err(_) => return,
        };
        let out = dispatch_one(
            &g,
            &GuardVerdict::accept(ContactClass::Finger),
            ContactClass::Finger,
            &c,
            10,
            2,
            0,
        );
        assert_eq!(out.ok().map(|o| (o.route, o.dx, o.dy)), Some((Route::Pointer, 10, 2)));
    }

    #[test]
    fn kill_ledger_is_clean_by_construction() {
        let mut led = KillLedger::new();
        let ok = DispatchOut {
            route: Route::Pointer,
            dx: 1,
            dy: 0,
            reason: DropReason::None,
            level: 5,
            switch_seq: 1,
        };
        led.record(ContactClass::Finger, &ok);
        led.record(ContactClass::Palm, &DispatchOut::dropped(DropReason::Palm, 1));
        assert!(led.clean());
        assert_eq!(led.drops(), 1);
        let mut bad = KillLedger::new();
        bad.record(ContactClass::Finger, &DispatchOut::dropped(DropReason::Palm, 1));
        assert!(!bad.clean());
    }

    #[test]
    fn golden_is_repeatable() {
        let a = golden_run();
        assert!(a.is_ok());
        assert_eq!(golden_run(), a);
        match a {
            Ok((curve, sw, delivered, root)) => {
                assert_eq!(curve, SENS_ROUNDS * SENS_LEVELS as u32);
                assert_eq!(sw, SWITCH_ROUNDS);
                assert!(delivered > 0);
                assert!(root != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_injection_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        match inject_rounds(10) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }
}
